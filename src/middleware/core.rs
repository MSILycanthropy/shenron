use std::ops::AsyncFnMut;

use crate::{Next, Result, Session};

/// A middleware that borrows the session and can act before and after the rest
/// of the chain.
///
/// The server owns the [`Session`] and lends each layer a `&mut Session`. A
/// middleware does its before-work, calls [`Next::run`] (re-lending the borrow
/// down the chain), then its after-work — still holding its `&mut`.
///
/// Implemented automatically for any `async` function or closure with the
/// signature `Fn(&mut Session, Next) -> Result`.
///
/// # Example
///
/// ```
/// use shenron::{Next, Result, Session};
///
/// async fn logging(session: &mut Session, next: Next<'_>) -> Result {
///     tracing::info!("{} connected", session.user());
///     next.run(session).await?;
///     tracing::info!("disconnected");
///     Ok(())
/// }
/// ```
pub trait Middleware: Send + Sync + 'static {
    fn handle<'a>(
        &'a self,
        session: &'a mut Session,
        next: Next<'a>,
    ) -> impl Future<Output = Result> + Send + 'a;
}

impl<F> Middleware for F
where
    F: AsyncFn(&mut Session, Next<'_>) -> Result + Send + Sync + 'static,
    for<'a> <F as AsyncFnMut<(&'a mut Session, Next<'a>)>>::CallRefFuture<'a>: Send,
{
    fn handle<'a>(
        &'a self,
        session: &'a mut Session,
        next: Next<'a>,
    ) -> impl Future<Output = Result> + Send + 'a {
        self(session, next)
    }
}

/// A terminal app adapted as middleware: it borrows the session, ignores
/// `next`, and is therefore always the innermost layer. Build it with
/// [`terminal`].
pub struct Terminal<F>(F);

/// Adapt a terminal app `Fn(&mut Session) -> Result` into a [`Middleware`]
/// that ignores the rest of the chain.
pub const fn terminal<F>(f: F) -> Terminal<F>
where
    F: AsyncFn(&mut Session) -> Result + Send + Sync + 'static,
    for<'a> <F as AsyncFnMut<(&'a mut Session,)>>::CallRefFuture<'a>: Send,
{
    Terminal(f)
}

impl<F> Middleware for Terminal<F>
where
    F: AsyncFn(&mut Session) -> Result + Send + Sync + 'static,
    for<'a> <F as AsyncFnMut<(&'a mut Session,)>>::CallRefFuture<'a>: Send,
{
    fn handle<'a>(
        &'a self,
        session: &'a mut Session,
        _next: Next<'a>,
    ) -> impl Future<Output = Result> + Send + 'a {
        (self.0)(session)
    }
}
