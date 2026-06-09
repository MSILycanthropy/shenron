use crate::{Auth, BoxFuture};

/// Type-erased password auth handler
pub(crate) trait PasswordAuth: Send + Sync {
    fn verify(&self, user: &str, password: &str) -> BoxFuture<Auth>;
}

impl<F, Fut> PasswordAuth for F
where
    F: Fn(String, String) -> Fut + Send + Sync,
    Fut: Future + Send + 'static,
    Fut::Output: Into<Auth>,
{
    fn verify(&self, user: &str, password: &str) -> BoxFuture<Auth> {
        let fut = (self)(user.to_string(), password.to_string());

        Box::pin(async move { fut.await.into() })
    }
}
