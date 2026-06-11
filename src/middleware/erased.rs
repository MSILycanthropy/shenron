use std::pin::Pin;

use crate::{Exit, IntoExit, Middleware, Next, Session};

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Type-erased middleware for storage in the server. The generic
/// [`Middleware::Output`] is erased to [`Exit`] here, at the boxing boundary.
pub(crate) trait ErasedMiddleware: Send + Sync {
    fn handle<'a>(&'a self, session: &'a mut Session, next: Next<'a>) -> BoxFuture<'a, Exit>;
}

impl<M: Middleware> ErasedMiddleware for M {
    fn handle<'a>(&'a self, session: &'a mut Session, next: Next<'a>) -> BoxFuture<'a, Exit> {
        Box::pin(async move { Middleware::handle(self, session, next).await.into_exit() })
    }
}

/// Type-erased handler for the middleware chain. Implemented only by the chain's
/// `Base` and `MiddlewareHandler` (see `chain.rs`).
pub(crate) trait ErasedHandler: Send + Sync {
    fn call<'a>(&'a self, session: &'a mut Session) -> BoxFuture<'a, Exit>;
}
