use std::{pin::Pin, sync::Arc};

use russh::{MethodKind, MethodSet, keys::PublicKey};

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

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

/// Configured authentication for a server
#[derive(Default, Clone)]
pub(crate) struct AuthConfig {
    pub password: Option<Arc<dyn PasswordAuth>>,
    pub pubkey: Option<Arc<dyn PubkeyAuth>>,
}

impl AuthConfig {
    pub fn is_empty(&self) -> bool {
        self.password.is_none() && self.pubkey.is_none()
    }

    pub fn methods(&self) -> MethodSet {
        let mut methods: Vec<MethodKind> = vec![];

        if self.password.is_some() {
            methods.push(MethodKind::Password);
        }

        if self.pubkey.is_some() {
            methods.push(MethodKind::PublicKey);
        }

        methods.as_slice().into()
    }
}
