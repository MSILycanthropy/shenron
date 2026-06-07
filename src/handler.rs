use std::future::Future;

use crate::{Result, Session};

/// A handler for SSH sessions.
///
/// This trait is automatically implemented for any function or closure
/// with the signature `Fn(Session) -> Future<Output = Result<Session>>`.
///
/// # Example
///
/// ```
/// use shenron::{Result, Session};
///
/// async fn my_app(mut session: Session) -> Result<Session> {
///     session.write_str("Hello!\r\n").await?;
///     session.exit(0)
/// }
/// ```
pub trait Handler: Send + Sync + Clone + 'static {
    type Future: Future<Output = Result<Session>> + Send;

    fn call(&self, session: Session) -> Self::Future;
}

impl<F, Fut> Handler for F
where
    F: Fn(Session) -> Fut + Send + Sync + Clone + 'static,
    Fut: Future<Output = Result<Session>> + Send,
{
    type Future = Fut;

    fn call(&self, session: Session) -> Self::Future {
        (self)(session)
    }
}
