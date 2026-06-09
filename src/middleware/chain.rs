use std::{pin::Pin, sync::Arc};

use crate::{
    Next, Result, Session,
    middleware::{ErasedHandler, ErasedMiddleware},
};

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub(crate) fn build_chain(middleware: Vec<Arc<dyn ErasedMiddleware>>) -> Arc<dyn ErasedHandler> {
    let mut chain: Arc<dyn ErasedHandler> = Arc::new(Base);

    for mw in middleware.into_iter().rev() {
        chain = Arc::new(MiddlewareHandler {
            middleware: mw,
            next: chain,
        });
    }

    chain
}

/// Terminates the chain: the innermost middleware's `next` bottoms out here.
struct Base;

impl ErasedHandler for Base {
    fn call<'a>(&'a self, _session: &'a mut Session) -> BoxFuture<'a, Result> {
        Box::pin(async { Ok(()) })
    }
}

struct MiddlewareHandler {
    middleware: Arc<dyn ErasedMiddleware>,
    next: Arc<dyn ErasedHandler>,
}

impl ErasedHandler for MiddlewareHandler {
    fn call<'a>(&'a self, session: &'a mut Session) -> BoxFuture<'a, Result> {
        let next = Next::new(self.next.as_ref());

        self.middleware.handle(session, next)
    }
}
