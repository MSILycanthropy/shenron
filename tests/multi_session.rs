//! End-to-end tests driving a real server with russh's client side: one
//! connection, multiple session channels — the OpenSSH `ControlMaster` shape.

#![feature(async_fn_traits, unboxed_closures)]

mod common;

use common::{Account, connect_and_auth, read_to_close, start_server};
use shenron::Session;

/// Echo back everything a session inherited, one field per fix under test.
async fn app(session: &mut Session) -> shenron::Result {
    let account = session.get::<Account>().map_or(0, |a| a.0);
    let command = session.raw_command().unwrap_or("none").to_owned();
    let argv0 = session
        .command()
        .and_then(|argv| argv.first().cloned())
        .unwrap_or_default();
    let pty = session.pty().is_some();
    let marker = session.env().get("MARKER").cloned().unwrap_or_default();

    session
        .write_str(&format!(
            "account={account};command={command};argv0={argv0};pty={pty};marker={marker}"
        ))
        .await?;

    Ok(())
}

#[tokio::test]
async fn second_session_keeps_auth_data_and_own_env() {
    let port = start_server(app).await;
    let handle = connect_and_auth(port).await;

    let mut first = handle.channel_open_session().await.expect("first channel");
    first.set_env(true, "MARKER", "one").await.expect("env");
    first.exec(true, "first").await.expect("exec");
    let out_first = read_to_close(&mut first).await;

    let mut second = handle.channel_open_session().await.expect("second channel");
    second.set_env(true, "MARKER", "two").await.expect("env");
    second.exec(true, "second").await.expect("exec");
    let out_second = read_to_close(&mut second).await;

    assert_eq!(
        out_first.stdout,
        "account=42;command=first;argv0=first;pty=false;marker=one"
    );
    assert_eq!(
        out_second.stdout,
        "account=42;command=second;argv0=second;pty=false;marker=two"
    );
}

#[tokio::test]
async fn pty_exec_keeps_the_command() {
    let port = start_server(app).await;
    let handle = connect_and_auth(port).await;

    let mut channel = handle.channel_open_session().await.expect("channel");
    channel
        .request_pty(true, "xterm", 80, 24, 0, 0, &[])
        .await
        .expect("pty");
    channel.exec(true, "deploy --prod").await.expect("exec");

    let out = read_to_close(&mut channel).await;

    assert_eq!(
        out.stdout,
        "account=42;command=deploy --prod;argv0=deploy;pty=true;marker="
    );
}
