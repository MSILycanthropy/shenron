use std::sync::Arc;

use russh::{MethodKind, MethodSet};

use crate::auth::{PasswordAuth, PubkeyAuth};

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
