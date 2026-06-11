use std::ops::AsyncFnMut;

use crate::{IntoExit, Next, Session};

/// A middleware that borrows the session and can act before and after the rest
/// of the chain.
///
/// The server owns the [`Session`] and lends each layer a `&mut Session`. A
/// middleware does its before-work, calls [`Next::run`] (re-lending the borrow
/// down the chain), then its after-work — still holding its `&mut`.
///
/// `Output` is anything [`IntoExit`]; return `next.run(session).await`'s
/// [`Exit`](crate::Exit) (usually wrapped in `Ok`) to pass the inner result
/// through.
///
/// Implemented automatically for any `async` function or closure with the
/// signature `Fn(&mut Session, Next) -> impl IntoExit`.
///
/// # Example
///
/// ```
/// use shenron::{Exit, Next, Result, Session};
///
/// async fn logging(session: &mut Session, next: Next<'_>) -> Result<Exit> {
///     tracing::info!("{} connected", session.user());
///     let exit = next.run(session).await;
///     tracing::info!("disconnected");
///     Ok(exit)
/// }
/// ```
pub trait Middleware: Send + Sync + 'static {
    type Output: IntoExit;

    fn handle<'a>(
        &'a self,
        session: &'a mut Session,
        next: Next<'a>,
    ) -> impl Future<Output = Self::Output> + Send + 'a;
}

impl<F, R> Middleware for F
where
    F: AsyncFn(&mut Session, Next<'_>) -> R + Send + Sync + 'static,
    for<'a> <F as AsyncFnMut<(&'a mut Session, Next<'a>)>>::CallRefFuture<'a>: Send,
    R: IntoExit,
{
    type Output = R;

    fn handle<'a>(
        &'a self,
        session: &'a mut Session,
        next: Next<'a>,
    ) -> impl Future<Output = R> + Send + 'a {
        self(session, next)
    }
}

/// A terminal app adapted as middleware: it borrows the session, ignores
/// `next`, and is therefore always the innermost layer. Build it with
/// [`terminal`].
pub struct Terminal<F>(F);

/// Adapt a terminal app `Fn(&mut Session) -> impl IntoExit` into a
/// [`Middleware`] that ignores the rest of the chain.
pub const fn terminal<F, R>(f: F) -> Terminal<F>
where
    F: AsyncFn(&mut Session) -> R + Send + Sync + 'static,
    for<'a> <F as AsyncFnMut<(&'a mut Session,)>>::CallRefFuture<'a>: Send,
    R: IntoExit,
{
    Terminal(f)
}

impl<F, R> Middleware for Terminal<F>
where
    F: AsyncFn(&mut Session) -> R + Send + Sync + 'static,
    for<'a> <F as AsyncFnMut<(&'a mut Session,)>>::CallRefFuture<'a>: Send,
    R: IntoExit,
{
    type Output = R;

    fn handle<'a>(
        &'a self,
        session: &'a mut Session,
        _next: Next<'a>,
    ) -> impl Future<Output = R> + Send + 'a {
        (self.0)(session)
    }
}
