// examples/graceful_shutdown.rs

use shenron::{Server, Session, SessionKind};

#[tokio::main]
async fn main() -> shenron::Result<()> {
    println!("Starting server on 127.0.0.1:2222");

    Server::new().bind("0.0.0.0:2222").app(app).serve().await?;

    println!("Server stopped");

    Ok(())
}

async fn app(mut session: Session) -> shenron::Result<Session> {
    match session.kind() {
        SessionKind::Subsystem { name } => match name.as_str() {
            "echo" => {
                while let Some(data) = session.input().await {
                    let s = String::from_utf8_lossy(&data);
                    session.write_str(&format!("Got: {s}\r\n")).await?;
                }
                session.exit(0)
            }
            other => {
                session
                    .write_stderr_str(&format!("Unknown subsystem: {other}\n"))
                    .await?;
                session.exit(1)
            }
        },
        SessionKind::Pty { .. } | SessionKind::Shell => {
            session
                .write_str("This server only supports subsystems.\r\n")
                .await?;
            session.write_str("Try: ssh -s echo\r\n").await?;
            session.exit(0)
        }
        SessionKind::Exec { command } => {
            session
                .write_stderr_str(&format!("Exec not supported: {command}\n"))
                .await?;
            session.exit(1)
        }
    }
}
