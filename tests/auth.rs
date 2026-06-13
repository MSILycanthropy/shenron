//! Auth method negotiation: open servers accept `none` like Wish; configured
//! servers reject it and advertise only the methods they actually answer.

#![feature(async_fn_traits, unboxed_closures)]

mod common;

use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

use common::{AcceptAll, start_open_server, start_server, start_server_with};
use russh::{
    MethodKind,
    client::{self, AuthResult},
    keys::{
        Algorithm, Certificate, PrivateKey, PrivateKeyWithHashAlg,
        ssh_key::certificate::{Builder, CertType},
    },
};
use shenron::Session;

async fn noop(_session: &mut Session) -> shenron::Result {
    Ok(())
}

async fn connect(port: u16) -> client::Handle<AcceptAll> {
    let config = Arc::new(client::Config::default());

    client::connect(config, ("127.0.0.1", port), AcceptAll)
        .await
        .expect("connect")
}

#[tokio::test]
async fn open_server_accepts_none_auth() {
    let port = start_open_server(noop).await;
    let mut handle = connect(port).await;

    let result = handle
        .authenticate_none("anyone")
        .await
        .expect("auth request");

    assert!(matches!(result, AuthResult::Success));
}

#[tokio::test]
async fn configured_server_rejects_none_and_advertises_real_methods() {
    let port = start_server(noop).await;
    let mut handle = connect(port).await;

    let result = handle
        .authenticate_none("anyone")
        .await
        .expect("auth request");

    let AuthResult::Failure {
        remaining_methods, ..
    } = result
    else {
        panic!("none auth must be rejected when auth is configured");
    };

    assert!(remaining_methods.contains(&MethodKind::Password));
    assert!(!remaining_methods.contains(&MethodKind::None));
    assert!(!remaining_methods.contains(&MethodKind::PublicKey));
    assert!(!remaining_methods.contains(&MethodKind::KeyboardInteractive));
}

fn generate() -> PrivateKey {
    PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("keygen")
}

fn write_lines(lines: &[String]) -> tempfile::NamedTempFile {
    let file = tempfile::NamedTempFile::new().expect("tempfile");
    std::fs::write(file.path(), lines.join("\n")).expect("write");
    file
}

/// A user cert for `principal`, signed by `ca`, valid for an hour.
fn sign_cert(ca: &PrivateKey, subject: &PrivateKey, principal: &str) -> Certificate {
    let now = SystemTime::now();

    let mut builder = Builder::new_with_validity_times(
        [0u8; 16],
        subject.public_key().key_data().clone(),
        now - Duration::from_secs(60),
        now + Duration::from_secs(3600),
    )
    .expect("builder");

    builder
        .cert_type(CertType::User)
        .expect("cert type")
        .valid_principal(principal)
        .expect("principal");

    builder.sign(ca).expect("sign")
}

#[tokio::test]
async fn authorized_keys_allows_listed_key_only() {
    let listed = generate();
    let file = write_lines(&[listed.public_key().to_openssh().expect("openssh")]);

    let port = start_server_with(noop, |server| {
        server.pubkey_auth(shenron::auth::authorized_keys(file.path()).expect("parse"))
    })
    .await;

    let mut handle = connect(port).await;
    let result = handle
        .authenticate_publickey("alice", PrivateKeyWithHashAlg::new(Arc::new(listed), None))
        .await
        .expect("auth request");
    assert!(matches!(result, AuthResult::Success));

    let mut handle = connect(port).await;
    let result = handle
        .authenticate_publickey(
            "alice",
            PrivateKeyWithHashAlg::new(Arc::new(generate()), None),
        )
        .await
        .expect("auth request");
    assert!(matches!(result, AuthResult::Failure { .. }));
}

#[tokio::test]
async fn trusted_ca_accepts_cert_for_principal_only() {
    let ca = generate();
    let subject = generate();
    let cert = sign_cert(&ca, &subject, "alice");
    let file = write_lines(&[ca.public_key().to_openssh().expect("openssh")]);

    let port = start_server_with(noop, |server| {
        server.cert_auth(shenron::auth::trusted_ca_keys(file.path()).expect("parse"))
    })
    .await;

    let mut handle = connect(port).await;
    let result = handle
        .authenticate_openssh_cert("alice", Arc::new(subject.clone()), cert.clone())
        .await
        .expect("auth request");
    assert!(matches!(result, AuthResult::Success));

    // Same valid cert, but presented for a username outside its principals.
    let mut handle = connect(port).await;
    let result = handle
        .authenticate_openssh_cert("mallory", Arc::new(subject), cert)
        .await
        .expect("auth request");
    assert!(matches!(result, AuthResult::Failure { .. }));
}

/// A cert-only server advertises `publickey` (certs ride that method on the
/// wire) but must still reject a plain public key — holding the key inside
/// the cert is not enough without the CA's signature.
#[tokio::test]
async fn cert_only_server_rejects_plain_pubkey() {
    let ca = generate();
    let subject = generate();
    let file = write_lines(&[ca.public_key().to_openssh().expect("openssh")]);

    let port = start_server_with(noop, |server| {
        server.cert_auth(shenron::auth::trusted_ca_keys(file.path()).expect("parse"))
    })
    .await;

    let mut handle = connect(port).await;
    let result = handle
        .authenticate_publickey("alice", PrivateKeyWithHashAlg::new(Arc::new(subject), None))
        .await
        .expect("auth request");

    let AuthResult::Failure {
        remaining_methods, ..
    } = result
    else {
        panic!("plain pubkey must be rejected by a cert-only server");
    };

    assert!(remaining_methods.contains(&MethodKind::PublicKey));
}
