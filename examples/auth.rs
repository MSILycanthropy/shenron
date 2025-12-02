use shenron::{Event, Result, Server, Session};

async fn whoami(mut session: Session) -> Result<()> {
    session
        .write_str(&format!(
            "Welcome {}! You're connected from {}\r\n",
            session.user(),
            session.remote_addr()
        ))
        .await?;

    session.write_str("Press any key to exit.\r\n").await?;

    while let Some(event) = session.next().await {
        match event {
            Event::Input(_) | Event::Eof => break,
            _ => {}
        }
    }

    session.write_str("Goodbye!\r\n").await?;
    session.exit(0).await?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let key =
        russh::keys::PrivateKey::random(&mut rand::rngs::OsRng, russh::keys::Algorithm::Ed25519)
            .expect("Failed to create key");

    let allowed_users = vec!["admin", "alice", "bob"];
    let admin_password = "supersecret";

    tracing::info!("Starting auth example on 0.0.0.0:2222");
    tracing::info!("Connect with: ssh -p 2222 admin@localhost");
    tracing::info!("Password: supersecret");

    Server::new()
        .bind("0.0.0.0:2222")
        .host_key(key)
        .password_auth(move |user, password| {
            let allowed = allowed_users.clone();
            let admin_pw = admin_password;
            async move {
                if !allowed.contains(&user.as_str()) {
                    tracing::warn!("Unknown user attempted login: {}", user);
                    return false;
                }

                if user == "admin" && password == admin_pw {
                    tracing::info!("Admin logged in with password");
                    return true;
                }

                tracing::warn!("Password auth failed for user: {}", user);
                false
            }
        })
        .app(whoami)
        .serve()
        .await
}
