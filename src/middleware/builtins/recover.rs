use std::{
    any::Any, future::poll_fn, net::SocketAddr, panic::AssertUnwindSafe, sync::Arc, task::Poll,
};

use tracing::error;

use crate::{Error, Exit, Middleware, Next, Session};

/// A panic caught while running the sub-chain, with the session context that was
/// captured before the session was borrowed into `next`.
struct Panicked {
    message: String,
    user: String,
    remote: SocketAddr,
}

/// Drive the rest of the chain, wrapping each `poll` in [`catch_unwind`] so a
/// panic becomes an `Err` instead of unwinding through us.
///
/// `Ok` carries the chain's own [`Exit`] (which may itself be an error); the
/// outer `Err` means the chain panicked.
///
/// [`catch_unwind`]: std::panic::catch_unwind
async fn guard(session: &mut Session, next: Next<'_>) -> Result<Exit, Panicked> {
    let user = session.user().to_owned();
    let remote = session.remote_addr();

    let mut fut = Box::pin(next.run(session));
    let outcome = poll_fn(|cx| {
        match std::panic::catch_unwind(AssertUnwindSafe(|| fut.as_mut().poll(cx))) {
            Ok(poll) => poll.map(Ok),
            Err(panic) => Poll::Ready(Err(panic)),
        }
    })
    .await;

    outcome.map_err(|panic| Panicked {
        message: panic_message(panic),
        user,
        remote,
    })
}

/// Downcast a panic payload to a readable message. Panics carrying a `&str` or
/// `String` (the common cases) are recovered verbatim; anything else is opaque.
fn panic_message(payload: Box<dyn Any + Send>) -> String {
    let payload = match payload.downcast::<&'static str>() {
        Ok(s) => return (*s).to_string(),
        Err(other) => other,
    };

    payload
        .downcast::<String>()
        .map_or_else(|_| "unknown panic payload".to_string(), |s| *s)
}

/// Middleware that contains a panicking handler or middleware instead of letting
/// it drop the session abruptly.
///
/// A panic in the wrapped chain is logged via `tracing` and converted into
/// [`Error::Panic`], so outer middleware still run their after-`next` logic and
/// the connection closes cleanly. The server and other sessions are unaffected.
///
/// Place it just inside your observability middleware (e.g.
/// `.with(logging).with(recover).app(your_app)`) so a panic becomes an
/// [`Exit::Error`] those outer layers can still observe.
pub async fn recover(session: &mut Session, next: Next<'_>) -> Exit {
    match guard(session, next).await {
        Ok(exit) => exit,
        Err(p) => {
            error!(user = %p.user, remote = %p.remote, panic = %p.message, "handler panicked");
            Exit::Error(Error::Panic(p.message))
        }
    }
}

/// Details of a caught panic, passed to a [`recover_with`] callback.
pub struct PanicReport<'a> {
    pub message: &'a str,
    pub user: &'a str,
    pub remote: SocketAddr,
}

/// Like [`recover`], but also invokes a callback on panic — for shipping the
/// panic to metrics, error reporting, etc. Build it with [`recover_with`].
#[derive(Clone)]
pub struct RecoverWith(Arc<dyn Fn(&PanicReport) + Send + Sync>);

/// Construct a [`RecoverWith`] middleware from a panic callback.
pub fn recover_with(callback: impl Fn(&PanicReport) + Send + Sync + 'static) -> RecoverWith {
    RecoverWith(Arc::new(callback))
}

impl Middleware for RecoverWith {
    type Output = Exit;

    async fn handle(&self, session: &'_ mut Session, next: Next<'_>) -> Exit {
        match guard(session, next).await {
            Ok(exit) => exit,
            Err(p) => {
                error!(user = %p.user, remote = %p.remote, panic = %p.message, "handler panicked");
                (self.0)(&PanicReport {
                    message: &p.message,
                    user: &p.user,
                    remote: p.remote,
                });
                Exit::Error(Error::Panic(p.message))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::panic_message;

    #[track_caller]
    fn payload_of(f: impl FnOnce() + std::panic::UnwindSafe) -> Box<dyn std::any::Any + Send> {
        let Err(payload) = std::panic::catch_unwind(f) else {
            panic!("closure did not panic");
        };
        payload
    }

    #[test]
    fn recovers_str_payload() {
        assert_eq!(panic_message(payload_of(|| panic!("boom"))), "boom");
    }

    #[test]
    fn recovers_string_payload() {
        let payload = payload_of(|| panic!("{}", String::from("dynamic")));
        assert_eq!(panic_message(payload), "dynamic");
    }

    #[test]
    fn opaque_for_non_string_payload() {
        let payload = payload_of(|| std::panic::panic_any(42_u32));
        assert_eq!(panic_message(payload), "unknown panic payload");
    }
}
