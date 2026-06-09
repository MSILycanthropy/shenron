use std::pin::Pin;

use crate::{Middleware, Next, Result, Session};

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Type-erased middleware for storage in the server.
pub(crate) trait ErasedMiddleware: Send + Sync {
    fn handle<'a>(&'a self, session: &'a mut Session, next: Next<'a>) -> BoxFuture<'a, Result>;
}

impl<M: Middleware> ErasedMiddleware for M {
    fn handle<'a>(&'a self, session: &'a mut Session, next: Next<'a>) -> BoxFuture<'a, Result> {
        Box::pin(Middleware::handle(self, session, next))
    }
}

/// Type-erased handler for the middleware chain. Implemented only by the chain's
/// `Base` and `MiddlewareHandler` (see `chain.rs`).
pub(crate) trait ErasedHandler: Send + Sync {
    fn call<'a>(&'a self, session: &'a mut Session) -> BoxFuture<'a, Result>;
}
