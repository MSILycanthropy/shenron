use crate::{Next, Result, Session};

/// A middleware that can wrap a handler.
///
/// Middleware can perform actions before and after the inner handler,
/// modify the session, short-circuit the chain, etc.
///
/// # Example
///
/// ```rust
/// use shenron::{Middleware, Next, Session, Result};
/// use std::time::Instant;
///
/// #[derive(Clone)]
/// struct LoggingMiddleware;
///
/// impl Middleware for LoggingMiddleware {
///     async fn handle(&self, session: Session, next: Next) -> Result<()> {
///         let user = session.user().to_owned();
///         tracing::info!("{} connected", user);
///
///         let result = next.run(session).await;
///
///         tracing::info!("{} disconnected", user);
///         result
///     }
/// }
/// ```
pub trait Middleware: Send + Sync + Clone + 'static {
    fn handle(&self, session: Session, next: Next) -> impl Future<Output = Result<Session>> + Send;
}

impl<F, Fut> Middleware for F
where
    F: Fn(Session, Next) -> Fut + Send + Sync + Clone + 'static,
    Fut: Future<Output = Result<Session>> + Send,
{
    fn handle(&self, session: Session, next: Next) -> impl Future<Output = Result<Session>> + Send {
        (self)(session, next)
    }
}
