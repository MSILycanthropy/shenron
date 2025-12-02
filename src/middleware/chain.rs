use std::sync::Arc;

use crate::{
    BoxFuture, Handler, Next, Result, Session,
    middleware::{ErasedHandler, ErasedMiddleware},
};

pub(crate) fn build_chain(
    handler: impl Handler,
    middlware: Vec<Arc<dyn ErasedMiddleware>>,
) -> Arc<dyn ErasedHandler> {
    let mut chain: Arc<dyn ErasedHandler> = Arc::new(handler);

    for mw in middlware.into_iter().rev() {
        chain = Arc::new(MiddlewareHandler {
            middleware: mw,
            next: chain,
        });
    }

    chain
}

struct MiddlewareHandler {
    middleware: Arc<dyn ErasedMiddleware>,
    next: Arc<dyn ErasedHandler>,
}

impl ErasedHandler for MiddlewareHandler {
    fn call(&self, session: Session) -> BoxFuture<Result<()>> {
        let next = Next::new(Arc::clone(&self.next));

        self.middleware.handle(session, next)
    }
}
