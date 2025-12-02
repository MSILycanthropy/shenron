use shenron::{Server, Session, SessionKind};
use std::fmt::Write;

#[tokio::main]
async fn main() -> shenron::Result<()> {
    let key =
        russh::keys::PrivateKey::random(&mut rand::rngs::OsRng, russh::keys::Algorithm::Ed25519)
            .expect("Failed to create key");

    Server::new()
        .bind("0.0.0.0:2222")
        .host_key(key)
        .app(app)
        .serve()
        .await
}

async fn app(mut session: Session) -> shenron::Result<()> {
    match session.kind() {
        SessionKind::Pty { term, size } => {
            session
                .write_str(&format!(
                    "Interactive session started\r\n\
                     Terminal: {}\r\n\
                     Size: {}x{}\r\n\
                     \r\n\
                     Type something (Ctrl+C to exit):\r\n",
                    term, size.width, size.height
                ))
                .await?;

            while let Some(data) = session.input().await {
                if data.contains(&3) {
                    // Ctrl+C
                    session.write_str("\r\nGoodbye!\r\n").await?;
                    break;
                }

                session.write_str("You typed: ").await?;
                session.write(&data).await?;
                session.write_str("\r\n").await?;
            }
        }
        SessionKind::Exec { command } => {
            // Simple command router
            let output = match command.trim() {
                "whoami" => format!("{}\n", session.user()),
                "date" => format!("{}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")),
                "uptime" => "up 0 days, mass hysteria\n".to_string(),
                "env" => {
                    let env = session.env();
                    if env.is_empty() {
                        "(no environment variables)\n".to_string()
                    } else {
                        env.iter().fold(String::new(), |mut acc, (k, v)| {
                            let _ = writeln!(acc, "{k}={v}");
                            acc
                        })
                    }
                }
                "help" => "Available commands: whoami, date, uptime, env, help\n".to_string(),
                other => format!("Unknown command: {other}\n"),
            };

            session.write_str(&output).await?;
        }
    }

    session.close().await
}
