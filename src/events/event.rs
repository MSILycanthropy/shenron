use crate::{PtySize, Signal};

/// An event delivered to an own-the-loop application.
///
/// This is the feature-independent counterpart to the raw [`crate::Event`]
/// produced by [`Session::next`](crate::Session::next). It adds an
/// [`App`](Event::App) variant carrying messages pushed through the channel
/// returned by [`Events::sender`](crate::events::Events::sender), so SSH input
/// and out-of-band application messages arrive through a single `match`.
///
/// `M` is the application message type and defaults to `()` for apps that do
/// not use server-side push.
#[derive(Debug)]
pub enum Event<M = ()> {
    /// Raw bytes read from the client.
    Input(Vec<u8>),
    /// The client's terminal was resized.
    Resize(PtySize),
    /// The client delivered a signal.
    Signal(Signal),
    /// A message pushed through [`Events::sender`](crate::events::Events::sender).
    App(M),
    /// The client sent EOF; no more input will arrive.
    Eof,
}

impl<M> From<crate::Event> for Event<M> {
    fn from(event: crate::Event) -> Self {
        match event {
            crate::Event::Input(data) => Self::Input(data),
            crate::Event::Resize(size) => Self::Resize(size),
            crate::Event::Signal(signal) => Self::Signal(signal),
            crate::Event::Eof => Self::Eof,
        }
    }
}
