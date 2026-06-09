//! End-to-end tests driving a real server with russh's client side: one
//! connection, multiple session channels — the OpenSSH `ControlMaster` shape.

use std::{sync::Arc, time::Duration};

use russh::{ChannelMsg, client, client::AuthResult, keys::PublicKey};
use shenron::{Auth, Server, Session};

#[derive(Clone)]
struct Account(u32);

/// Echo back everything a session inherited, one field per fix under test.
async fn app(session: &mut Session) -> shenron::Result {
    let account = session.get::<Account>().map_or(0, |a| a.0);
    let command = session.command().unwrap_or("none").to_owned();
    let pty = session.pty().is_some();
    let marker = session.env().get("MARKER").cloned().unwrap_or_default();

    session
        .write_str(&format!(
            "account={account};command={command};pty={pty};marker={marker}"
        ))
        .await?;

    session.exit(0)
}

async fn start_server() -> u16 {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind probe")
        .local_addr()
        .expect("local addr")
        .port();

    let tmp = tempfile::TempDir::new().expect("tempdir");

    let server = Server::new()
        .bind(format!("127.0.0.1:{port}"))
        .host_key_path(tmp.path().join("host_key"))
        .expect("host key")
        .password_auth(|_user, _password| async { Auth::accept().with(Account(42)) })
        .app(app);

    tokio::spawn(server.serve());

    for _ in 0..100 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            return port;
        }

        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    panic!("server did not start listening");
}

struct AcceptAll;

impl client::Handler for AcceptAll {
    type Error = russh::Error;

    async fn check_server_key(&mut self, _key: &PublicKey) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

async fn connect_and_auth(port: u16) -> client::Handle<AcceptAll> {
    let config = Arc::new(client::Config::default());
    let mut handle = client::connect(config, ("127.0.0.1", port), AcceptAll)
        .await
        .expect("connect");

    let result = handle
        .authenticate_password("alice", "hunter2")
        .await
        .expect("auth request");
    assert!(matches!(result, AuthResult::Success));

    handle
}

/// Collect stdout until the server closes the channel.
async fn read_to_close(channel: &mut russh::Channel<client::Msg>) -> String {
    let mut out = Vec::new();

    while let Some(msg) = channel.wait().await {
        if let ChannelMsg::Data { data } = msg {
            out.extend_from_slice(&data);
        }
    }

    String::from_utf8_lossy(&out).into_owned()
}

#[tokio::test]
async fn second_session_keeps_auth_data_and_own_env() {
    let port = start_server().await;
    let handle = connect_and_auth(port).await;

    let mut first = handle.channel_open_session().await.expect("first channel");
    first.set_env(true, "MARKER", "one").await.expect("env");
    first.exec(true, "first").await.expect("exec");
    let out_first = read_to_close(&mut first).await;

    let mut second = handle
        .channel_open_session()
        .await
        .expect("second channel");
    second.set_env(true, "MARKER", "two").await.expect("env");
    second.exec(true, "second").await.expect("exec");
    let out_second = read_to_close(&mut second).await;

    assert_eq!(out_first, "account=42;command=first;pty=false;marker=one");
    assert_eq!(out_second, "account=42;command=second;pty=false;marker=two");
}

#[tokio::test]
async fn pty_exec_keeps_the_command() {
    let port = start_server().await;
    let handle = connect_and_auth(port).await;

    let mut channel = handle.channel_open_session().await.expect("channel");
    channel
        .request_pty(true, "xterm", 80, 24, 0, 0, &[])
        .await
        .expect("pty");
    channel.exec(true, "deploy --prod").await.expect("exec");

    let out = read_to_close(&mut channel).await;

    assert_eq!(out, "account=42;command=deploy --prod;pty=true;marker=");
}
