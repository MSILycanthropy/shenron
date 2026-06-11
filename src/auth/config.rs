use std::sync::Arc;

use russh::{MethodKind, MethodSet};

use crate::auth::{PasswordAuth, PubkeyAuth};

/// Configured authentication for a server
#[derive(Default, Clone)]
pub struct AuthConfig {
    pub password: Option<Arc<dyn PasswordAuth>>,
    pub pubkey: Option<Arc<dyn PubkeyAuth>>,
}

impl AuthConfig {
    pub fn is_empty(&self) -> bool {
        self.password.is_none() && self.pubkey.is_none()
    }

    /// The auth methods this server actually answers — never russh's
    /// default set, which advertises methods we always reject
    /// (keyboard-interactive, hostbased).
    ///
    /// An open server (no handlers) accepts `none`; password and publickey
    /// stay advertised for clients that skip `none`.
    pub fn methods(&self) -> MethodSet {
        if self.is_empty() {
            let all = [
                MethodKind::None,
                MethodKind::Password,
                MethodKind::PublicKey,
            ];

            return all.as_slice().into();
        }

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
