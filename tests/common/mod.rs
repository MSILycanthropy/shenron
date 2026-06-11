//! Shared harness: a real shenron server on a free port, driven by russh's
//! client side.
//!
//! Compiled separately into each test binary, and no binary uses everything.
#![allow(dead_code)]

use std::{sync::Arc, time::Duration};

use russh::{ChannelMsg, client, client::AuthResult, keys::PublicKey};
use shenron::{Auth, Server, Session};

/// Attached during auth; sessions read it back to prove auth data survived.
#[derive(Clone)]
pub struct Account(pub u32);

pub async fn start_server<F, R>(app: F) -> u16
where
    F: AsyncFn(&mut Session) -> R + Send + Sync + 'static,
    for<'a> <F as std::ops::AsyncFnMut<(&'a mut Session,)>>::CallRefFuture<'a>: Send,
    R: shenron::IntoExit,
{
    start(app, true).await
}

/// Like [`start_server`] but with no auth configured — the server is open.
pub async fn start_open_server<F, R>(app: F) -> u16
where
    F: AsyncFn(&mut Session) -> R + Send + Sync + 'static,
    for<'a> <F as std::ops::AsyncFnMut<(&'a mut Session,)>>::CallRefFuture<'a>: Send,
    R: shenron::IntoExit,
{
    start(app, false).await
}

async fn start<F, R>(app: F, with_auth: bool) -> u16
where
    F: AsyncFn(&mut Session) -> R + Send + Sync + 'static,
    for<'a> <F as std::ops::AsyncFnMut<(&'a mut Session,)>>::CallRefFuture<'a>: Send,
    R: shenron::IntoExit,
{
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind probe")
        .local_addr()
        .expect("local addr")
        .port();

    let tmp = tempfile::TempDir::new().expect("tempdir");

    let mut server = Server::new()
        .bind(format!("127.0.0.1:{port}"))
        .host_key_path(tmp.path().join("host_key"))
        .expect("host key");

    if with_auth {
        server =
            server.password_auth(|_user, _password| async { Auth::accept().with(Account(42)) });
    }

    tokio::spawn(server.app(app).serve());

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

pub struct AcceptAll;

impl client::Handler for AcceptAll {
    type Error = russh::Error;

    async fn check_server_key(&mut self, _key: &PublicKey) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

pub async fn connect_and_auth(port: u16) -> client::Handle<AcceptAll> {
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

pub struct Output {
    pub stdout: String,
    pub exit_status: Option<u32>,
}

/// Collect stdout and the exit status until the server closes the channel.
/// Bounded by a timeout so a server that never closes fails the test instead
/// of hanging it.
pub async fn read_to_close(channel: &mut russh::Channel<client::Msg>) -> Output {
    let mut stdout = Vec::new();
    let mut exit_status = None;

    let drain = async {
        while let Some(msg) = channel.wait().await {
            match msg {
                ChannelMsg::Data { data } => stdout.extend_from_slice(&data),
                ChannelMsg::ExitStatus { exit_status: code } => exit_status = Some(code),
                _ => {}
            }
        }
    };

    tokio::time::timeout(Duration::from_secs(2), drain)
        .await
        .expect("server never closed the channel");

    Output {
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        exit_status,
    }
}
