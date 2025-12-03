use std::{thread::sleep, time::Duration};

use shenron::{
    Result, Server, Session,
    middleware::builtins::{Comment, elapsed},
};

async fn sleep_and_die(session: Session) -> Result<Session> {
    session.write_str("Welcome to Shenron!\r\n").await?;

    sleep(Duration::from_secs(1));

    session.exit(0)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!("Starting echo server on 0.0.0.0:2222");
    tracing::info!("Connect with: ssh -p 2222 localhost");

    Server::new()
        .bind("0.0.0.0:2222")
        .with(Comment("Cya! Wouldn't wanna be ya!\r\n".into()))
        .with(elapsed)
        .app(sleep_and_die)
        .serve()
        .await
}
