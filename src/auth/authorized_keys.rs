use std::{collections::HashSet, future::Ready, path::Path};

use russh::keys::{
    PublicKey,
    ssh_key::{AuthorizedKeys, public::KeyData},
};

/// A ready-made pubkey handler, accepted by
/// [`pubkey_auth`](crate::server::Server::pubkey_auth) like any closure.
pub type PubkeyHandler = Box<dyn Fn(String, PublicKey) -> Ready<bool> + Send + Sync>;

/// Build a pubkey handler that accepts only keys listed in an OpenSSH
/// `authorized_keys` file.
///
/// The file is read once, here; edits require a restart. Keys are compared by
/// key material, so comments and per-line options don't affect matching. Like
/// Wish's `WithAuthorizedKeys`, the allowlist is server-wide — the username is
/// not consulted.
///
/// Unlike sshd, quoted option values containing spaces (e.g.
/// `command="echo hi"`) are not supported and fail parsing — here at startup,
/// not silently per-login.
///
/// ```no_run
/// # use shenron::Server;
/// # fn main() -> shenron::Result<()> {
/// let _server = Server::new()
///     .pubkey_auth(shenron::auth::authorized_keys(".ssh/authorized_keys")?);
/// # Ok(())
/// # }
/// ```
///
/// # Errors
///
/// Returns `Err` if the file cannot be read or parsed.
pub fn authorized_keys(path: impl AsRef<Path>) -> crate::Result<PubkeyHandler> {
    let keys: HashSet<KeyData> = AuthorizedKeys::read_file(path.as_ref())?
        .into_iter()
        .map(|entry| entry.public_key().key_data().clone())
        .collect();

    Ok(Box::new(move |_user: String, key: PublicKey| {
        std::future::ready(keys.contains(key.key_data()))
    }))
}

#[cfg(test)]
mod tests {
    use russh::keys::{Algorithm, PrivateKey};

    use super::*;

    fn generate() -> PublicKey {
        PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
            .expect("keygen")
            .public_key()
            .clone()
    }

    fn write_authorized_keys(lines: &[String]) -> tempfile::NamedTempFile {
        let file = tempfile::NamedTempFile::new().expect("tempfile");
        std::fs::write(file.path(), lines.join("\n")).expect("write");
        file
    }

    #[tokio::test]
    async fn accepts_listed_key_and_rejects_unlisted() {
        let listed = generate();
        let file = write_authorized_keys(&[listed.to_openssh().expect("openssh")]);

        let handler = authorized_keys(file.path()).expect("parse");

        assert!(handler("alice".into(), listed).await);
        assert!(!handler("alice".into(), generate()).await);
    }

    #[tokio::test]
    async fn matches_despite_options_and_comments() {
        let listed = generate();
        let line = format!(
            "command=\"uptime\",no-pty {} someone@example.com",
            listed.to_openssh().expect("openssh")
        );
        let file = write_authorized_keys(&["# a comment".into(), String::new(), line]);

        let handler = authorized_keys(file.path()).expect("parse");

        assert!(handler("alice".into(), listed).await);
    }

    #[test]
    fn quoted_value_with_space_errors_at_startup() {
        let line = format!(
            "command=\"echo hi\" {}",
            generate().to_openssh().expect("openssh")
        );
        let file = write_authorized_keys(&[line]);

        assert!(authorized_keys(file.path()).is_err());
    }

    #[test]
    fn missing_file_errors() {
        assert!(authorized_keys("/nonexistent/authorized_keys").is_err());
    }
}
