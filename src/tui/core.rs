use std::{collections::VecDeque, io::Write};

use ratatui::{
    Frame, Terminal as RatatuiTerminal, TerminalOptions, Viewport, layout::Rect,
    prelude::CrosstermBackend,
};
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    Error, Result, Session,
    events::{Event as RawEvent, Events},
    tui::{
        event::Event,
        key::{Input, parse_input},
        writer::SessionWriter,
    },
};

type Backend = CrosstermBackend<SessionWriter>;

/// A terminal UI session: drives a `ratatui` terminal over the SSH channel,
/// merging key input with pushed application messages.
///
/// Built via [`Session::tui`](crate::Session::tui). Borrows the session for the
/// duration of the loop; call [`close`](Self::close) to restore the terminal
/// and release the borrow.
pub struct Tui<'a, M = ()> {
    events: Events<'a, M>,
    terminal: RatatuiTerminal<Backend>,
    /// Keys still queued from the last input packet — one packet can carry
    /// many keys (paste, fast typing), delivered one per [`next`](Self::next).
    pending: VecDeque<Input>,
    alt_screen: bool,
    entered: bool,
}

impl<'a, M> Tui<'a, M> {
    pub(crate) fn new(session: &'a mut Session) -> Result<Self> {
        let Some(pty_size) = session.pty_size() else {
            return Err(Error::Protocol("tui requires a pty".into()));
        };

        let area: Rect = pty_size.try_into()?;
        let terminal = RatatuiTerminal::with_options(
            CrosstermBackend::new(SessionWriter::new()),
            TerminalOptions {
                viewport: Viewport::Fixed(area),
            },
        )?;

        Ok(Self {
            events: Events::new(session),
            terminal,
            pending: VecDeque::new(),
            alt_screen: false,
            entered: false,
        })
    }

    /// Render on the alternate screen, restoring the client's prior screen on
    /// [`close`](Self::close). Off by default (inline rendering).
    #[must_use]
    pub const fn alt_screen(mut self) -> Self {
        self.alt_screen = true;
        self
    }

    /// A `'static` sender for pushing [`App`](Event::App) messages into the
    /// loop from spawned tasks.
    #[must_use]
    pub fn sender(&self) -> UnboundedSender<M> {
        self.events.sender()
    }

    /// Write raw data to the client, bypassing the terminal.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the message fails to send.
    pub async fn write(&self, data: &[u8]) -> Result {
        self.events.write(data).await
    }

    /// Write a raw string to the client, bypassing the terminal.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the message fails to send.
    pub async fn write_str(&self, s: &str) -> Result {
        self.events.write_str(s).await
    }

    /// Render a frame, then flush the resulting bytes to the client.
    ///
    /// The cursor follows `frame.cursor_position`, set during `render`.
    ///
    /// # Errors
    ///
    /// Returns `Err` if rendering or the write fails.
    pub async fn draw(&mut self, render: impl FnOnce(&mut Frame)) -> Result {
        if self.alt_screen && !self.entered {
            self.terminal
                .backend_mut()
                .writer_mut()
                .write_all(b"\x1b[?1049h")?;
            self.entered = true;
        }

        self.terminal.draw(render)?;

        let data = self.terminal.backend_mut().writer_mut().take();
        self.events.write(&data).await
    }

    /// Await the next event, parsing input into keys and pastes and resizing
    /// the terminal in step with the client. Unparseable input, mouse
    /// reports, and signals are skipped.
    pub async fn next(&mut self) -> Option<Event<M>> {
        loop {
            if let Some(input) = self.pending.pop_front() {
                return Some(match input {
                    Input::Key(key) => Event::Key(key),
                    Input::Paste(text) => Event::Paste(text),
                });
            }

            match self.events.next().await? {
                RawEvent::Input(bytes) => self.pending.extend(parse_input(&bytes)),
                RawEvent::Resize(size) => {
                    if let Ok(rect) = size.try_into() {
                        let _ = self.terminal.resize(rect);
                    }
                    return Some(Event::Resize(size));
                }
                RawEvent::Signal(_) => {}
                RawEvent::App(msg) => return Some(Event::App(msg)),
                RawEvent::Eof => return Some(Event::Eof),
            }
        }
    }

    /// Restore terminal state (show cursor, leave the alternate screen if
    /// entered) and release the session borrow.
    ///
    /// Required before exit when [`alt_screen`](Self::alt_screen) is on, else
    /// the client is left on the alternate screen.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the write fails.
    pub async fn close(self) -> Result {
        let mut restore: Vec<u8> = b"\x1b[?25h".to_vec();

        if self.alt_screen && self.entered {
            restore.extend_from_slice(b"\x1b[?1049l");
        } else {
            restore.extend_from_slice(b"\r\n");
        }

        self.events.write(&restore).await
    }
}
