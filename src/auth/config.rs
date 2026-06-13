use std::sync::Arc;

use russh::{MethodKind, MethodSet};

use crate::auth::{CertAuth, KeyboardInteractiveAuth, PasswordAuth, PubkeyAuth};

/// Configured authentication for a server
#[derive(Default, Clone)]
pub struct AuthConfig {
    pub password: Option<Arc<dyn PasswordAuth>>,
    pub pubkey: Option<Arc<dyn PubkeyAuth>>,
    pub cert: Option<Arc<dyn CertAuth>>,
    pub keyboard_interactive: Option<Arc<dyn KeyboardInteractiveAuth>>,
}

impl AuthConfig {
    pub fn is_empty(&self) -> bool {
        self.password.is_none()
            && self.pubkey.is_none()
            && self.cert.is_none()
            && self.keyboard_interactive.is_none()
    }

    /// The auth methods this server actually answers — never russh's default
    /// set, which advertises `hostbased` (always rejected) and
    /// keyboard-interactive even when no handler is configured.
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

        // Certificates ride the publickey method on the wire, so a cert-only
        // server still advertises (and only answers) `publickey`.
        if self.pubkey.is_some() || self.cert.is_some() {
            methods.push(MethodKind::PublicKey);
        }

        if self.keyboard_interactive.is_some() {
            methods.push(MethodKind::KeyboardInteractive);
        }

        methods.as_slice().into()
    }
}

#[cfg(test)]
mod tests {
    use russh::keys::Certificate;

    use super::*;

    #[test]
    fn cert_only_config_advertises_publickey() {
        let config = AuthConfig {
            cert: Some(Arc::new(|_user: String, _cert: Certificate| async { true })),
            ..AuthConfig::default()
        };

        assert!(!config.is_empty());

        let methods = config.methods();

        assert!(methods.contains(&MethodKind::PublicKey));
        assert!(!methods.contains(&MethodKind::Password));
        assert!(!methods.contains(&MethodKind::None));
    }
}
