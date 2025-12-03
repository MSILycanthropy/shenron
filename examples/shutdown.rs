// examples/graceful_shutdown.rs

use shenron::{Server, Session, SessionKind};

#[tokio::main]
async fn main() -> shenron::Result<()> {
    println!("Starting server on 127.0.0.1:2222");
    println!("Press Ctrl+C to shut down gracefully");

    Server::new()
        .bind("0.0.0.0:2222")
        .shutdown_signal(async {
            tokio::signal::ctrl_c().await.ok();
            println!("\nShutdown signal received, stopping server...");
        })
        .app(app)
        .serve()
        .await?;

    println!("Server stopped");

    Ok(())
}

async fn app(mut session: Session) -> shenron::Result<Session> {
    match session.kind() {
        SessionKind::Pty { .. } | SessionKind::Shell => {
            session
                .write_str("Connected! Server may shut down at any time.\r\n")
                .await?;
            session
                .write_str("Type anything, Ctrl+C to exit:\r\n")
                .await?;

            while let Some(data) = session.input().await {
                if data.contains(&3) {
                    break;
                }
                session.write(&data).await?;
            }

            session.write_str("\r\nGoodbye!\r\n").await?;
        }
        SessionKind::Exec { command } => {
            session.write_str(&format!("Executed: {command}\n")).await?;
        }
        SessionKind::Subsystem { .. } => {}
    }

    session.exit(0)
}
