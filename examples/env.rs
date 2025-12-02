use shenron::{Server, Session};

#[tokio::main]
async fn main() -> shenron::Result<()> {
    Server::new()
        .bind("127.0.0.1:2222")
        .host_key_file("host_key")?
        .app(app)
        .serve()
        .await
}

async fn app(session: Session) -> shenron::Result<()> {
    session.write_str("Environment variables:\r\n").await?;
    session.write_str("----------------------\r\n").await?;

    let env = session.env();

    if env.is_empty() {
        session.write_str("(none received)\r\n").await?;
        session
            .write_str("\r\nTip: use `ssh -o SendEnv=FOO` to send variables\r\n")
            .await?;
    } else {
        for (key, value) in env {
            session.write_str(&format!("  {key}={value}\r\n")).await?;
        }
    }

    session.write_str("\r\nSession info:\r\n").await?;
    session
        .write_str(&format!("  TERM={}\r\n", session.term()))
        .await?;
    session
        .write_str(&format!("  USER={}\r\n", session.user()))
        .await?;
    session
        .write_str(&format!("  REMOTE={}\r\n", session.remote_addr()))
        .await?;

    session.close().await
}
