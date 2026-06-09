use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::{Result, Session, events::Event};

/// An own-the-loop session handle: one [`next`](Self::next) call merges SSH
/// input with application messages pushed through [`sender`](Self::sender).
///
/// Borrows the session for the duration of the loop; drop it to use the session
/// again.
pub struct Events<'a, M = ()> {
    session: &'a mut Session,
    sender: UnboundedSender<M>,
    receiver: UnboundedReceiver<M>,
}

impl<'a, M> Events<'a, M> {
    pub(crate) fn new(session: &'a mut Session) -> Self {
        let (sender, receiver) = unbounded_channel();

        Self {
            session,
            sender,
            receiver,
        }
    }

    /// Await the next event, merging SSH input and pushed application messages.
    ///
    /// Cancel-safe: both the SSH read and the channel receive are cancel-safe,
    /// so dropping a pending `next` loses nothing.
    pub async fn next(&mut self) -> Option<Event<M>> {
        tokio::select! {
            event = self.session.next() => event.map(Into::into),
            Some(msg) = self.receiver.recv() => Some(Event::App(msg)),
        }
    }

    /// A `'static` sender for pushing [`App`](Event::App) messages into the
    /// loop from spawned tasks.
    #[must_use]
    pub fn sender(&self) -> UnboundedSender<M> {
        self.sender.clone()
    }

    /// Write data to the client.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the message fails to send.
    pub async fn write(&self, data: &[u8]) -> Result {
        self.session.write(data).await
    }

    /// Write a string to the client.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the message fails to send.
    pub async fn write_str(&self, s: &str) -> Result {
        self.session.write_str(s).await
    }
}
