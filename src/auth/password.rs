use crate::BoxFuture;

/// Type-erased password auth handler
pub(crate) trait PasswordAuth: Send + Sync {
    fn verify(&self, user: &str, password: &str) -> BoxFuture<bool>;
}

impl<F, Fut> PasswordAuth for F
where
    F: Fn(String, String) -> Fut + Send + Sync,
    Fut: Future<Output = bool> + Send + 'static,
{
    fn verify(&self, user: &str, password: &str) -> BoxFuture<bool> {
        let fut = (self)(user.to_string(), password.to_string());

        Box::pin(fut)
    }
}
