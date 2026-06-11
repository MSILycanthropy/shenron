//! Auth method negotiation: open servers accept `none` like Wish; configured
//! servers reject it and advertise only the methods they actually answer.

#![feature(async_fn_traits, unboxed_closures)]

mod common;

use std::sync::Arc;

use common::{AcceptAll, start_open_server, start_server};
use russh::{
    MethodKind,
    client::{self, AuthResult},
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
