use russh::keys::PublicKey;

use crate::BoxFuture;

/// Type erased pubkey auth handler
pub(crate) trait PubkeyAuth: Send + Sync {
    fn verify(&self, user: &str, key: &PublicKey) -> BoxFuture<bool>;
}

impl<F, Fut> PubkeyAuth for F
where
    F: Fn(String, PublicKey) -> Fut + Send + Sync,
    Fut: Future<Output = bool> + Send + 'static,
{
    fn verify(&self, user: &str, key: &PublicKey) -> BoxFuture<bool> {
        let fut = (self)(user.to_string(), key.clone());
        Box::pin(fut)
    }
}
