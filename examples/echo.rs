use shenron::{Event, Next, Result, Server, Session};

async fn echo(mut session: Session) -> Result<Session> {
    session.write_str("Welcome to Shenron!\r\n").await?;
    session
        .write_str(&format!("Hello, {}!\r\n", session.user()))
        .await?;
    session
        .write_str("Type anything and it will be echoed back.\r\n")
        .await?;
    session
        .write_str("Press Ctrl+C or Ctrl+D to exit.\r\n\r\n")
        .await?;

    while let Some(event) = session.next().await {
        match event {
            Event::Input(data) => {
                if data.contains(&3) || data.contains(&4) {
                    session.write_str("\r\nGoodbye!\r\n").await?;
                    break;
                }

                let s = String::from_utf8_lossy(&data);
                session.write_str(&format!("Got: {s}\r\n")).await?;
            }
            Event::Resize(size) => {
                tracing::debug!("Resized to {}x{}", size.width, size.height);
            }
            Event::Eof => break,
            Event::Signal(_) => {}
        }
    }

    session.exit(0)
}

async fn log(session: Session, next: Next) -> Result<Session> {
    tracing::info!(
        "{} connected from {}",
        session.user(),
        session.remote_addr()
    );
    let result = next.run(session).await;
    tracing::info!("session ended");
    result
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
        .with(log)
        .app(echo)
        .serve()
        .await
}
