/// Transforms or drops events before the application loop sees them.
///
/// Returning `Some` passes the (possibly transformed) event through; `None`
/// drops it. Applied inline by the app, e.g. `let Some(ev) = fx.apply(ev) else
/// { continue };`. Generic over the event type, so it works for both
/// [`events::Event`](crate::events::Event) and `tui::Event`.
pub trait Interceptor<E> {
    fn apply(&mut self, event: E) -> Option<E>;
}

impl<E, F: FnMut(E) -> Option<E>> Interceptor<E> for F {
    fn apply(&mut self, event: E) -> Option<E> {
        self(event)
    }
}

/// A chain of [`Interceptor`]s applied in order; the first to drop an event
/// short-circuits the rest.
#[derive(Default)]
pub struct Interceptors<E>(Vec<Box<dyn Interceptor<E> + Send>>);

impl<E> Interceptors<E> {
    #[must_use]
    pub fn new() -> Self {
        Self(Vec::new())
    }

    #[must_use]
    pub fn with(mut self, interceptor: impl Interceptor<E> + Send + 'static) -> Self {
        self.0.push(Box::new(interceptor));
        self
    }
}

impl<E> Interceptor<E> for Interceptors<E> {
    fn apply(&mut self, mut event: E) -> Option<E> {
        for interceptor in &mut self.0 {
            event = interceptor.apply(event)?;
        }

        Some(event)
    }
}
