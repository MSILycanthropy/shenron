use shenron::{Server, Session};

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

async fn app(session: Session) -> shenron::Result<Session> {
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
        .write_str(&format!(
            "  TERM={}\r\n",
            session.term().expect("Not a pty session")
        ))
        .await?;
    session
        .write_str(&format!("  USER={}\r\n", session.user()))
        .await?;
    session
        .write_str(&format!("  REMOTE={}\r\n", session.remote_addr()))
        .await?;

    session.exit(0)
}
