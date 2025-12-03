use std::pin::Pin;

use ratatui::{
    Frame, Terminal,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
    prelude::CrosstermBackend,
};

use crate::{Event, Handler, PtySize, Result, Session, tui::writer::SessionWriter};

/// Trait for ratatui apps
pub trait App: Send + Sync + Clone + 'static {
    /// Initialize app state
    fn init(&mut self) {}

    /// Handle key events, return false to exit
    fn handle_key(&mut self, key: KeyEvent) -> bool;

    /// Handle a resize event
    fn handle_resize(&mut self, _size: PtySize) {}

    /// Draw a frame of the UI
    fn draw(&self, frame: &mut Frame);
}

/// Wrapper struct around a Ratatui app to run it
#[derive(Clone)]
pub struct Ratatui<A: App> {
    pub app: A,
}

impl<A> Handler for Ratatui<A>
where
    A: App,
{
    type Future = Pin<Box<dyn Future<Output = Result<Session>> + Send>>;

    fn call(&self, session: crate::Session) -> Self::Future {
        let app = self.app.clone();

        Box::pin(async move {
            let Some(pty_size) = session.pty_size() else {
                return Ok(session);
            };

            let mut app = app;

            app.init();

            run_app(session, app, pty_size).await
        })
    }
}

type RemoteTerminal<'a> = Terminal<CrosstermBackend<SessionWriter>>;

async fn run_app<A: App>(mut session: Session, mut app: A, pty_size: PtySize) -> Result<Session> {
    let writer = SessionWriter::new();
    let backend = CrosstermBackend::new(writer);
    let mut terminal = RemoteTerminal::new(backend)?;

    terminal.resize(pty_size.try_into()?)?;

    terminal.hide_cursor()?;
    terminal.clear()?;

    let data = terminal.backend_mut().writer_mut().take();
    session.write(&data).await?;

    loop {
        terminal.draw(|frame| app.draw(frame))?;

        let data = terminal.backend_mut().writer_mut().take();
        session.write(&data).await?;

        let Some(event) = session.next().await else {
            break;
        };

        match event {
            Event::Input(data) => {
                if let Some(key) = parse_key_event(&data)
                    && !app.handle_key(key)
                {
                    break;
                }
            }
            Event::Resize(new_size) => {
                terminal.resize(new_size.try_into()?)?;
                app.handle_resize(new_size);
            }
            Event::Eof => break,
            Event::Signal(_) => {}
        }
    }

    session.exit(0)
}

#[must_use]
pub fn parse_key_event(data: &[u8]) -> Option<KeyEvent> {
    if data.is_empty() {
        return None;
    }

    let key = match data {
        // Arrow keys
        [27, 91, 65] => KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
        [27, 91, 66] => KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        [27, 91, 67] => KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
        [27, 91, 68] => KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),

        // Arrow keys with modifiers (some terminals)
        [27, 91, 49, 59, 50, 65] => KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT),
        [27, 91, 49, 59, 50, 66] => KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT),
        [27, 91, 49, 59, 50, 67] => KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT),
        [27, 91, 49, 59, 50, 68] => KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT),
        [27, 91, 49, 59, 53, 65] => KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL),
        [27, 91, 49, 59, 53, 66] => KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL),
        [27, 91, 49, 59, 53, 67] => KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL),
        [27, 91, 49, 59, 53, 68] => KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL),

        // Home/End
        [27, 91 | 79, 72] => KeyEvent::new(KeyCode::Home, KeyModifiers::NONE),
        [27, 91 | 79, 70] => KeyEvent::new(KeyCode::End, KeyModifiers::NONE),

        // Insert/Delete/PageUp/PageDown
        [27, 91, 50, 126] => KeyEvent::new(KeyCode::Insert, KeyModifiers::NONE),
        [27, 91, 51, 126] => KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE),
        [27, 91, 53, 126] => KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE),
        [27, 91, 54, 126] => KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE),

        // Function keys
        [27, 79, 80] => KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE),
        [27, 79, 81] => KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE),
        [27, 79, 82] => KeyEvent::new(KeyCode::F(3), KeyModifiers::NONE),
        [27, 79, 83] => KeyEvent::new(KeyCode::F(4), KeyModifiers::NONE),
        [27, 91, 49, 53, 126] => KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE),
        [27, 91, 49, 55, 126] => KeyEvent::new(KeyCode::F(6), KeyModifiers::NONE),
        [27, 91, 49, 56, 126] => KeyEvent::new(KeyCode::F(7), KeyModifiers::NONE),
        [27, 91, 49, 57, 126] => KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE),
        [27, 91, 50, 48, 126] => KeyEvent::new(KeyCode::F(9), KeyModifiers::NONE),
        [27, 91, 50, 49, 126] => KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE),
        [27, 91, 50, 51, 126] => KeyEvent::new(KeyCode::F(11), KeyModifiers::NONE),
        [27, 91, 50, 52, 126] => KeyEvent::new(KeyCode::F(12), KeyModifiers::NONE),

        // Escape
        [27] => KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),

        // Enter
        [13] => KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),

        // Tab / Shift+Tab
        [9] => KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        [27, 91, 90] => KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),

        // Backspace
        [127 | 8] => KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),

        // Space
        [32] => KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),

        // Ctrl+letter (Ctrl+A = 1, Ctrl+B = 2, ..., Ctrl+Z = 26)
        [b @ 1..=26] => {
            let c = (b'a' + b - 1) as char;
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
        }

        // Regular ASCII character
        [c] if c.is_ascii_graphic() || *c == b' ' => {
            let c = *c as char;
            if c.is_ascii_uppercase() {
                KeyEvent::new(KeyCode::Char(c.to_ascii_lowercase()), KeyModifiers::SHIFT)
            } else {
                KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
            }
        }

        // Alt+letter (ESC followed by letter)
        [27, c] if c.is_ascii_alphabetic() => {
            let c = (*c as char).to_ascii_lowercase();
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT)
        }

        _ => return parse_utf8_char(data),
    };

    Some(key)
}

fn parse_utf8_char(data: &[u8]) -> Option<KeyEvent> {
    let s = std::str::from_utf8(data).ok()?;

    let c = s.chars().next()?;

    let modifiers = if c.is_ascii_uppercase() {
        KeyModifiers::SHIFT
    } else {
        KeyModifiers::NONE
    };

    let c = if c.is_ascii_uppercase() {
        c.to_ascii_lowercase()
    } else {
        c
    };

    Some(KeyEvent::new(KeyCode::Char(c), modifiers))
}
