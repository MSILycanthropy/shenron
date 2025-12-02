use std::sync::Arc;

use crate::{Result, Session, middleware::ErasedHandler};

/// The next handler in the middleware chain.
pub struct Next {
    inner: Arc<dyn ErasedHandler>,
}

impl Next {
    pub(crate) fn new(handler: Arc<dyn ErasedHandler>) -> Self {
        Self { inner: handler }
    }

    /// Run the next middlware in the chain
    ///
    /// # Errors
    ///
    /// Returns `Err` if the next middleware fails
    pub async fn run(self, session: Session) -> Result<()> {
        self.inner.call(session).await
    }
}
