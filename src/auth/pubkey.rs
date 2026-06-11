use russh::keys::PublicKey;

use crate::{Auth, BoxFuture};

/// Type erased pubkey auth handler
pub trait PubkeyAuth: Send + Sync {
    fn verify(&self, user: &str, key: &PublicKey) -> BoxFuture<Auth>;
}

impl<F, Fut> PubkeyAuth for F
where
    F: Fn(String, PublicKey) -> Fut + Send + Sync,
    Fut: Future + Send + 'static,
    Fut::Output: Into<Auth>,
{
    fn verify(&self, user: &str, key: &PublicKey) -> BoxFuture<Auth> {
        let fut = (self)(user.to_string(), key.clone());
        Box::pin(async move { fut.await.into() })
    }
}
