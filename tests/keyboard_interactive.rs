//! End-to-end keyboard-interactive auth: the challenge-response conversation,
//! multi-round flows, attached extensions, method advertisement, and surviving
//! a client that disconnects mid-challenge.

#![feature(async_fn_traits, unboxed_closures)]

mod common;

use std::sync::Arc;

use common::{AcceptAll, Account, read_to_close, start_server_with};
use russh::{
    MethodKind,
    client::{self, AuthResult, KeyboardInteractiveAuthResponse as Kbi},
};
use shenron::{Auth, Session, auth::Prompt};

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
async fn single_round_accepts_correct_answer() {
    let port = start_server_with(noop, |server| {
        server.keyboard_interactive_auth(|user, mut ch| async move {
            let answers = ch.challenge("", "", [Prompt::hidden("code: ")]).await?;

            Ok(Auth::from(user == "alice" && answers[0] == "1234"))
        })
    })
    .await;

    let mut handle = connect(port).await;

    let resp = handle
        .authenticate_keyboard_interactive_start("alice", None::<String>)
        .await
        .expect("start");

    let Kbi::InfoRequest { prompts, .. } = resp else {
        panic!("expected an info request, got {resp:?}");
    };
    assert_eq!(prompts.len(), 1);
    assert!(!prompts[0].echo, "a hidden prompt must not echo");

    let resp = handle
        .authenticate_keyboard_interactive_respond(vec!["1234".into()])
        .await
        .expect("respond");

    assert!(matches!(resp, Kbi::Success));
}

#[tokio::test]
async fn single_round_rejects_wrong_answer() {
    let port = start_server_with(noop, |server| {
        server.keyboard_interactive_auth(|_user, mut ch| async move {
            let answers = ch.challenge("", "", [Prompt::hidden("code: ")]).await?;

            Ok(Auth::from(answers[0] == "1234"))
        })
    })
    .await;

    let mut handle = connect(port).await;

    handle
        .authenticate_keyboard_interactive_start("alice", None::<String>)
        .await
        .expect("start");

    let resp = handle
        .authenticate_keyboard_interactive_respond(vec!["nope".into()])
        .await
        .expect("respond");

    let Kbi::Failure {
        remaining_methods, ..
    } = resp
    else {
        panic!("a wrong answer must be rejected, got {resp:?}");
    };
    assert!(remaining_methods.contains(&MethodKind::KeyboardInteractive));
}

#[tokio::test]
async fn multi_round_conversation() {
    let port = start_server_with(noop, |server| {
        server.keyboard_interactive_auth(|_user, mut ch| async move {
            let name = ch.challenge("", "", [Prompt::echo("username: ")]).await?;
            let secret = ch.challenge("", "", [Prompt::hidden("password: ")]).await?;

            Ok(Auth::from(name[0] == "admin" && secret[0] == "s3cret"))
        })
    })
    .await;

    let mut handle = connect(port).await;

    let resp = handle
        .authenticate_keyboard_interactive_start("admin", None::<String>)
        .await
        .expect("start");
    let Kbi::InfoRequest { prompts, .. } = resp else {
        panic!("expected first round, got {resp:?}");
    };
    assert!(prompts[0].echo, "username prompt should echo");

    let resp = handle
        .authenticate_keyboard_interactive_respond(vec!["admin".into()])
        .await
        .expect("first answer");
    let Kbi::InfoRequest { prompts, .. } = resp else {
        panic!("expected second round, got {resp:?}");
    };
    assert!(!prompts[0].echo, "password prompt should not echo");

    let resp = handle
        .authenticate_keyboard_interactive_respond(vec!["s3cret".into()])
        .await
        .expect("second answer");
    assert!(matches!(resp, Kbi::Success));
}

async fn echo_account(session: &mut Session) -> shenron::Result {
    let account = session.get::<Account>().map_or(0, |a| a.0);

    session.write_str(&format!("account={account}")).await?;

    Ok(())
}

#[tokio::test]
async fn accept_attaches_extension_to_session() {
    let port = start_server_with(echo_account, |server| {
        server.keyboard_interactive_auth(|_user, mut ch| async move {
            ch.challenge("", "", [Prompt::hidden("code: ")]).await?;

            Ok(Auth::accept().with(Account(7)))
        })
    })
    .await;

    let mut handle = connect(port).await;

    handle
        .authenticate_keyboard_interactive_start("alice", None::<String>)
        .await
        .expect("start");
    let resp = handle
        .authenticate_keyboard_interactive_respond(vec!["x".into()])
        .await
        .expect("respond");
    assert!(matches!(resp, Kbi::Success));

    let mut channel = handle.channel_open_session().await.expect("channel");
    channel.exec(true, "whoami").await.expect("exec");
    let out = read_to_close(&mut channel).await;

    assert_eq!(out.stdout, "account=7");
}

#[tokio::test]
async fn kbi_only_server_advertises_only_keyboard_interactive() {
    let port = start_server_with(noop, |server| {
        server.keyboard_interactive_auth(|_user, mut ch| async move {
            ch.challenge("", "", [Prompt::hidden("code: ")]).await?;

            Ok(Auth::reject())
        })
    })
    .await;

    let mut handle = connect(port).await;

    let result = handle
        .authenticate_none("anyone")
        .await
        .expect("auth request");

    let AuthResult::Failure {
        remaining_methods, ..
    } = result
    else {
        panic!("none auth must be rejected when kbi is configured");
    };

    assert!(remaining_methods.contains(&MethodKind::KeyboardInteractive));
    assert!(!remaining_methods.contains(&MethodKind::None));
    assert!(!remaining_methods.contains(&MethodKind::Password));
    assert!(!remaining_methods.contains(&MethodKind::PublicKey));
}

/// A client that opens a conversation and vanishes before answering must not
/// wedge the server: the handler task unwinds when its `Challenger` drops, and
/// the next connection authenticates normally.
#[tokio::test]
async fn server_survives_disconnect_mid_challenge() {
    let port = start_server_with(noop, |server| {
        server.keyboard_interactive_auth(|_user, mut ch| async move {
            ch.challenge("", "", [Prompt::hidden("code: ")]).await?;

            Ok(Auth::accept())
        })
    })
    .await;

    let mut handle = connect(port).await;
    let resp = handle
        .authenticate_keyboard_interactive_start("alice", None::<String>)
        .await
        .expect("start");
    assert!(matches!(resp, Kbi::InfoRequest { .. }));
    drop(handle);

    let mut handle = connect(port).await;
    handle
        .authenticate_keyboard_interactive_start("alice", None::<String>)
        .await
        .expect("start");
    let resp = handle
        .authenticate_keyboard_interactive_respond(vec!["x".into()])
        .await
        .expect("respond");
    assert!(matches!(resp, Kbi::Success));
}
