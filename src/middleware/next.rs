use crate::{Result, Session, middleware::ErasedHandler};

/// The next handler in the middleware chain.
pub struct Next<'a> {
    inner: &'a dyn ErasedHandler,
}

impl<'a> Next<'a> {
    pub(crate) const fn new(inner: &'a dyn ErasedHandler) -> Self {
        Self { inner }
    }

    /// Run the next middleware in the chain.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the next middleware fails.
    pub async fn run(self, session: &mut Session) -> Result {
        self.inner.call(session).await
    }
}
