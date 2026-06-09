//! A session channel must close when the handler returns, however it returns.
//! Regression tests for handlers that hang the client (sh-v6x.4).

#![feature(async_fn_traits, unboxed_closures)]

mod common;

use common::{connect_and_auth, read_to_close, start_server};
use shenron::Session;

/// Returning `Ok(())` without calling `exit()` reports success.
async fn returns_without_exit(session: &mut Session) -> shenron::Result {
    session.write_str("done").await?;

    Ok(())
}

async fn returns_error(_session: &mut Session) -> shenron::Result {
    Err(shenron::Error::Protocol("app blew up".into()))
}

async fn exits_nonzero(session: &mut Session) -> shenron::Result {
    session.exit(3)
}

#[tokio::test]
async fn ok_without_exit_closes_with_status_zero() {
    let port = start_server(returns_without_exit).await;
    let handle = connect_and_auth(port).await;

    let mut channel = handle.channel_open_session().await.expect("channel");
    channel.exec(true, "anything").await.expect("exec");

    let out = read_to_close(&mut channel).await;

    assert_eq!(out.stdout, "done");
    assert_eq!(out.exit_status, Some(0));
}

#[tokio::test]
async fn handler_error_closes_with_status_one() {
    let port = start_server(returns_error).await;
    let handle = connect_and_auth(port).await;

    let mut channel = handle.channel_open_session().await.expect("channel");
    channel.exec(true, "anything").await.expect("exec");

    let out = read_to_close(&mut channel).await;

    assert_eq!(out.exit_status, Some(1));
}

#[tokio::test]
async fn explicit_exit_code_is_reported() {
    let port = start_server(exits_nonzero).await;
    let handle = connect_and_auth(port).await;

    let mut channel = handle.channel_open_session().await.expect("channel");
    channel.exec(true, "anything").await.expect("exec");

    let out = read_to_close(&mut channel).await;

    assert_eq!(out.exit_status, Some(3));
}
