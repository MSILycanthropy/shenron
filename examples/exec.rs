use shenron::{Server, Session, SessionKind};
use std::fmt::Write;

#[tokio::main]
async fn main() -> shenron::Result<()> {
    Server::new().bind("0.0.0.0:2222").app(app).serve().await
}

async fn app(session: Session) -> shenron::Result<Session> {
    if let SessionKind::Exec { command } = session.kind() {
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
            other => {
                session
                    .write_stderr_str(&format!("Unknown command: {other}\n"))
                    .await?;
                return session.exit(127);
            }
        };

        session.write_str(&output).await?;
    }

    session.exit(0)
}
