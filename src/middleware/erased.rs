use crate::{BoxFuture, Handler, Middleware, Next, Result, Session};

/// Type-erased Handler for the middleware chain
pub(crate) trait ErasedHandler: Send + Sync {
    fn call(&self, session: Session) -> BoxFuture<Result<Session>>;
}

impl<H: Handler> ErasedHandler for H {
    fn call(&self, session: Session) -> BoxFuture<Result<Session>> {
        Box::pin(Handler::call(self, session))
    }
}

/// Type-erased middleware for storage in the server
pub(crate) trait ErasedMiddleware: Send + Sync {
    fn handle(&self, session: Session, next: Next) -> BoxFuture<Result<Session>>;
}

impl<M: Middleware> ErasedMiddleware for M {
    fn handle(&self, session: Session, next: Next) -> BoxFuture<Result<Session>> {
        let this = self.clone();

        Box::pin(async move { Middleware::handle(&this, session, next).await })
    }
}
