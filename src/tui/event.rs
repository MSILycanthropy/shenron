use ratatui::crossterm::event::KeyEvent;

use crate::PtySize;

/// An event delivered to a terminal UI loop.
///
/// The TUI counterpart to [`events::Event`](crate::events::Event): SSH input is
/// parsed into a [`Key`](Event::Key), and signals are not surfaced. `M` is the
/// application message type, defaulting to `()`.
#[derive(Debug)]
pub enum Event<M = ()> {
    /// A parsed key press.
    Key(KeyEvent),
    /// The client's terminal was resized; the [`Tui`](crate::tui::Tui) has
    /// already resized its terminal.
    Resize(PtySize),
    /// A message pushed through [`Tui::sender`](crate::tui::Tui::sender).
    App(M),
    /// The client sent EOF; no more input will arrive.
    Eof,
}
