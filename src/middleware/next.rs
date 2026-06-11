use crate::{Exit, Session, middleware::ErasedHandler};

/// The next handler in the middleware chain.
pub struct Next<'a> {
    inner: &'a dyn ErasedHandler,
}

impl<'a> Next<'a> {
    pub(crate) const fn new(inner: &'a dyn ErasedHandler) -> Self {
        Self { inner }
    }

    /// Run the next middleware in the chain, resolving its return value to
    /// an [`Exit`]. Failures arrive as [`Exit::Error`] rather than `Err`, so
    /// callers inspect rather than `?`.
    pub async fn run(self, session: &mut Session) -> Exit {
        self.inner.call(session).await
    }
}
