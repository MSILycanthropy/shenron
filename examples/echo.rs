use async_trait::async_trait;
use shenron::{App, Result, Session};

#[derive(Default)]
struct EchoServer;

#[async_trait]
impl App for EchoServer {
    async fn handle(&self, mut session: Session) -> Result<()> {
        session.write_str("Welcome to Shrenron!\r\n").await?;
        session
            .write_str(&format!("Hello, {}!\r\n", session.user))
            .await?;
        session
            .write_str("Type anything and it will be echoed back.\r\n")
            .await?;
        session
            .write_str("Press Ctrl+C or Ctrl+D to exit.\r\n\r\n")
            .await?;

        while let Some(data) = session.read().await {
            if data.contains(&3) || data.contains(&4) {
                session.write_str("\r\nGoodbye!\r\n").await?;
                session.end().await?;
                break;
            }

            let string = String::from_utf8(data.clone()).unwrap_or_else(|_| "?".into());

            session
                .write_str(&format!("\r\nGot Data: {string}\r\n"))
                .await?;

            if session.has_resized() {
                let size = session.pty_size();
                tracing::debug!("Window resized to {}x{}", size.width, size.height);
            }
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let host_key =
        russh::keys::PrivateKey::random(&mut rand::rngs::OsRng, russh::keys::Algorithm::Ed25519)
            .expect("Failed to create key");

    tracing::info!("Starting echo server on 0.0.0.0:2222");
    tracing::info!("Connect with: ssh -p 2222 localhost");

    shenron::Server::default()
        .bind("0.0.0.0:2222")
        .host_key(host_key)
        .app(EchoServer)
        .serve()
        .await?;

    Ok(())
}
