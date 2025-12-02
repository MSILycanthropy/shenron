use std::{pin::Pin, sync::Arc};

use crate::Result;
use crate::Session;
use crate::handler::Handler;

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

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
    fn handle(&self, session: Session, next: Next) -> impl Future<Output = Result<()>> + Send;
}

impl<F, Fut> Middleware for F
where
    F: Fn(Session, Next) -> Fut + Send + Sync + Clone + 'static,
    Fut: Future<Output = Result<()>> + Send,
{
    fn handle(&self, session: Session, next: Next) -> impl Future<Output = Result<()>> + Send {
        (self)(session, next)
    }
}

/// Type-erased Handler for the middleware chain
pub(crate) trait ErasedHandler: Send + Sync {
    fn call(&self, session: Session) -> BoxFuture<Result<()>>;
}

impl<H: Handler> ErasedHandler for H {
    fn call(&self, session: Session) -> BoxFuture<Result<()>> {
        Box::pin(Handler::call(self, session))
    }
}

/// Type-erased middleware for storage in the server
pub(crate) trait ErasedMiddleware: Send + Sync {
    fn handle(&self, session: Session, next: Next) -> BoxFuture<Result<()>>;
}

impl<M: Middleware> ErasedMiddleware for M {
    fn handle(&self, session: Session, next: Next) -> BoxFuture<Result<()>> {
        let this = self.clone();

        Box::pin(async move { Middleware::handle(&this, session, next).await })
    }
}

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
