use shenron::{Result, Server, Session, sftp::Sftp};

async fn shell(session: &mut Session) -> Result {
    session
        .write_str("This server only speaks SFTP. Try: sftp -P 2222 localhost\r\n")
        .await?;

    session.exit(0)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let root = std::env::temp_dir();

    tracing::info!("Serving {} on 0.0.0.0:2222", root.display());
    tracing::info!("Connect with: sftp -P 2222 localhost");

    Server::new()
        .bind("0.0.0.0:2222")
        .with(Sftp::local(&root))
        .app(shell)
        .serve()
        .await
}
