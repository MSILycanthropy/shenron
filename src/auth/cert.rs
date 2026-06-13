use russh::keys::Certificate;

use crate::{Auth, BoxFuture};

/// Type erased certificate auth handler
pub trait CertAuth: Send + Sync {
    fn verify(&self, user: &str, cert: &Certificate) -> BoxFuture<Auth>;
}

impl<F, Fut> CertAuth for F
where
    F: Fn(String, Certificate) -> Fut + Send + Sync,
    Fut: Future + Send + 'static,
    Fut::Output: Into<Auth>,
{
    fn verify(&self, user: &str, cert: &Certificate) -> BoxFuture<Auth> {
        let fut = (self)(user.to_string(), cert.clone());
        Box::pin(async move { fut.await.into() })
    }
}
