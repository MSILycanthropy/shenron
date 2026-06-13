use tokio::sync::{mpsc, oneshot};

use crate::{Auth, BoxFuture};

/// A single line shown to the user during a keyboard-interactive challenge.
///
/// Use [`echo`](Self::echo) for visible input (a username) and
/// [`hidden`](Self::hidden) for secrets (a password, an OTP code).
pub struct Prompt {
    pub(crate) text: String,
    pub(crate) echo: bool,
}

impl Prompt {
    /// A prompt whose typed characters are visible.
    pub fn echo(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            echo: true,
        }
    }

    /// A prompt whose typed characters are masked.
    pub fn hidden(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            echo: false,
        }
    }
}

/// One round of a keyboard-interactive conversation, carried from the handler
/// to the server bridge. The bridge fires `reply` with the client's answers.
pub struct Challenge {
    pub name: String,
    pub instructions: String,
    pub prompts: Vec<Prompt>,
    pub reply: oneshot::Sender<Vec<String>>,
}

/// Handle passed to a keyboard-interactive handler for driving the
/// challenge-response conversation imperatively.
///
/// Each [`challenge`](Self::challenge) sends prompts to the client and waits
/// for the answers, so a handler reads as straight-line async code regardless
/// of how many rounds it runs.
pub struct Challenger {
    tx: mpsc::Sender<Challenge>,
}

impl Challenger {
    /// Send a round of prompts and await the client's answers, one string per
    /// prompt in order.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Protocol`](crate::Error::Protocol) if the client
    /// disconnects before answering.
    pub async fn challenge(
        &mut self,
        name: impl Into<String>,
        instructions: impl Into<String>,
        prompts: impl IntoIterator<Item = Prompt>,
    ) -> crate::Result<Vec<String>> {
        let (reply, answers) = oneshot::channel();

        let challenge = Challenge {
            name: name.into(),
            instructions: instructions.into(),
            prompts: prompts.into_iter().collect(),
            reply,
        };

        self.tx.send(challenge).await.map_err(|_| disconnected())?;

        answers.await.map_err(|_| disconnected())
    }
}

fn disconnected() -> crate::Error {
    crate::Error::Protocol("keyboard-interactive client disconnected".into())
}

/// Wire a [`Challenger`] to its receiving end. The handler keeps the
/// `Challenger`; the server bridge drives the receiver.
pub fn channel() -> (Challenger, mpsc::Receiver<Challenge>) {
    let (tx, rx) = mpsc::channel(1);

    (Challenger { tx }, rx)
}

/// Type-erased keyboard-interactive auth handler
pub trait KeyboardInteractiveAuth: Send + Sync {
    fn verify(&self, user: &str, challenger: Challenger) -> BoxFuture<crate::Result<Auth>>;
}

impl<F, Fut> KeyboardInteractiveAuth for F
where
    F: Fn(String, Challenger) -> Fut + Send + Sync,
    Fut: Future<Output = crate::Result<Auth>> + Send + 'static,
{
    fn verify(&self, user: &str, challenger: Challenger) -> BoxFuture<crate::Result<Auth>> {
        Box::pin((self)(user.to_string(), challenger))
    }
}
