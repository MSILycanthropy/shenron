use std::{future::Ready, path::Path};

use russh::keys::{
    Certificate, HashAlg, PublicKey,
    ssh_key::{Fingerprint, certificate::CertType},
};

/// A ready-made certificate handler, accepted by
/// [`cert_auth`](crate::server::Server::cert_auth) like any closure.
pub type CertHandler = Box<dyn Fn(String, Certificate) -> Ready<bool> + Send + Sync>;

/// Build a certificate handler that accepts user certs signed by a CA listed
/// in the given file (sshd's `TrustedUserCAKeys` format: one public key per
/// line, `#` comments).
///
/// A certificate is accepted iff all of:
///
/// - it is a *user* certificate — host certs from the same CA are not login
///   credentials
/// - it validates against a listed CA (signature + validity window)
/// - the username is listed in its principals; certs with *no* principals
///   are rejected, as sshd does for `TrustedUserCAKeys` — "valid for
///   everyone" must be an explicit decision, so handle it with a custom
///   [`cert_auth`](crate::server::Server::cert_auth) closure if you mean it
/// - it carries no critical options: none are enforced here yet, and the
///   spec says unrecognized critical options must fail, not be ignored
///
/// ```no_run
/// # use shenron::Server;
/// # fn main() -> shenron::Result<()> {
/// let _server = Server::new()
///     .cert_auth(shenron::auth::trusted_ca_keys("ca.pub")?);
/// # Ok(())
/// # }
/// ```
///
/// # Errors
///
/// Returns `Err` if the file cannot be read or parsed, or lists no keys.
pub fn trusted_ca_keys(path: impl AsRef<Path>) -> crate::Result<CertHandler> {
    let fingerprints = parse(&std::fs::read_to_string(path.as_ref())?)?;

    Ok(Box::new(move |user: String, cert: Certificate| {
        std::future::ready(check(&user, &cert, &fingerprints))
    }))
}

fn parse(input: &str) -> crate::Result<Vec<Fingerprint>> {
    let mut fingerprints = vec![];

    for (n, line) in input.lines().enumerate() {
        let line = line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let key: PublicKey = line.parse().map_err(|err| {
            crate::Error::Config(format!("trusted CA keys line {}: {err}", n + 1))
        })?;

        fingerprints.push(key.fingerprint(HashAlg::Sha256));
    }

    if fingerprints.is_empty() {
        return Err(crate::Error::Config(
            "trusted CA keys file lists no keys".into(),
        ));
    }

    Ok(fingerprints)
}

fn check(user: &str, cert: &Certificate, fingerprints: &[Fingerprint]) -> bool {
    cert.cert_type() == CertType::User
        && cert.validate(fingerprints).is_ok()
        && cert.valid_principals().iter().any(|p| p == user)
        && cert.critical_options().is_empty()
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use russh::keys::{
        Algorithm, PrivateKey,
        ssh_key::certificate::{Builder, CertType},
    };

    use super::*;

    fn generate() -> PrivateKey {
        PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("keygen")
    }

    fn write_ca_file(keys: &[&PrivateKey]) -> tempfile::NamedTempFile {
        let file = tempfile::NamedTempFile::new().expect("tempfile");
        let lines: Vec<String> = keys
            .iter()
            .map(|k| k.public_key().to_openssh().expect("openssh"))
            .collect();
        std::fs::write(file.path(), lines.join("\n")).expect("write");
        file
    }

    fn cert_builder(subject: &PrivateKey, cert_type: CertType) -> Builder {
        let now = SystemTime::now();

        let mut builder = Builder::new_with_validity_times(
            [0u8; 16],
            subject.public_key().key_data().clone(),
            now - Duration::from_secs(60),
            now + Duration::from_secs(3600),
        )
        .expect("builder");

        builder
            .cert_type(cert_type)
            .expect("cert type")
            .valid_principal("alice")
            .expect("principal");

        builder
    }

    fn handler_for(ca: &PrivateKey) -> CertHandler {
        let file = write_ca_file(&[ca]);

        trusted_ca_keys(file.path()).expect("parse")
    }

    #[tokio::test]
    async fn accepts_valid_user_cert() {
        let ca = generate();
        let cert = cert_builder(&generate(), CertType::User)
            .sign(&ca)
            .expect("sign");

        assert!(handler_for(&ca)("alice".into(), cert).await);
    }

    #[tokio::test]
    async fn rejects_cert_from_unlisted_ca() {
        let cert = cert_builder(&generate(), CertType::User)
            .sign(&generate())
            .expect("sign");

        assert!(!handler_for(&generate())("alice".into(), cert).await);
    }

    #[tokio::test]
    async fn rejects_host_cert() {
        let ca = generate();
        let cert = cert_builder(&generate(), CertType::Host)
            .sign(&ca)
            .expect("sign");

        assert!(!handler_for(&ca)("alice".into(), cert).await);
    }

    #[tokio::test]
    async fn rejects_principal_mismatch() {
        let ca = generate();
        let cert = cert_builder(&generate(), CertType::User)
            .sign(&ca)
            .expect("sign");

        assert!(!handler_for(&ca)("mallory".into(), cert).await);
    }

    #[tokio::test]
    async fn rejects_cert_without_principals() {
        let ca = generate();
        let now = SystemTime::now();

        let mut builder = Builder::new_with_validity_times(
            [0u8; 16],
            generate().public_key().key_data().clone(),
            now - Duration::from_secs(60),
            now + Duration::from_secs(3600),
        )
        .expect("builder");
        builder
            .cert_type(CertType::User)
            .expect("cert type")
            .all_principals_valid()
            .expect("principals");
        let cert = builder.sign(&ca).expect("sign");

        assert!(!handler_for(&ca)("alice".into(), cert).await);
    }

    #[tokio::test]
    async fn rejects_cert_with_critical_option() {
        let ca = generate();
        let mut builder = cert_builder(&generate(), CertType::User);
        builder
            .critical_option("force-command", "uptime")
            .expect("option");
        let cert = builder.sign(&ca).expect("sign");

        assert!(!handler_for(&ca)("alice".into(), cert).await);
    }

    #[tokio::test]
    async fn rejects_expired_cert() {
        let ca = generate();
        let now = SystemTime::now();

        let mut builder = Builder::new_with_validity_times(
            [0u8; 16],
            generate().public_key().key_data().clone(),
            now - Duration::from_secs(7200),
            now - Duration::from_secs(3600),
        )
        .expect("builder");
        builder
            .cert_type(CertType::User)
            .expect("cert type")
            .valid_principal("alice")
            .expect("principal");
        let cert = builder.sign(&ca).expect("sign");

        assert!(!handler_for(&ca)("alice".into(), cert).await);
    }

    #[tokio::test]
    async fn accepts_cert_from_second_listed_ca() {
        let (ca1, ca2) = (generate(), generate());
        let cert = cert_builder(&generate(), CertType::User)
            .sign(&ca2)
            .expect("sign");
        let file = write_ca_file(&[&ca1, &ca2]);

        let handler = trusted_ca_keys(file.path()).expect("parse");

        assert!(handler("alice".into(), cert).await);
    }

    #[test]
    fn empty_file_errors() {
        let file = write_ca_file(&[]);

        assert!(trusted_ca_keys(file.path()).is_err());
    }
}
