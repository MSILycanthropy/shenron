use std::sync::atomic::{AtomicU64, Ordering};

use shenron::{Auth, Next, Result, Server, Session};

/// Attached during pubkey auth, read back by the app.
#[derive(Clone)]
struct Account {
    id: u32,
    role: &'static str,
}

/// Inserted by middleware. A newtype so it can't collide with another stored
/// `String` on the same `TypeId`.
struct RequestId(String);

static REQUESTS: AtomicU64 = AtomicU64::new(0);

/// Tag each session with a unique request id before the app runs.
async fn request_id(session: &mut Session, next: Next<'_>) -> Result {
    let n = REQUESTS.fetch_add(1, Ordering::Relaxed);
    session.insert(RequestId(format!("req-{n}")));

    next.run(session).await
}

async fn app(session: &mut Session) -> Result {
    // Clone out so the immutable borrows end before we write.
    let account = session.get::<Account>().cloned();
    let request = session.get::<RequestId>().map(|r| r.0.clone());

    match account {
        Some(account) => {
            session
                .write_str(&format!(
                    "Hello account #{} ({})\r\n",
                    account.id, account.role
                ))
                .await?;
        }
        None => session.write_str("No account attached\r\n").await?,
    }

    if let Some(request) = request {
        session
            .write_str(&format!("Request id: {request}\r\n"))
            .await?;
    }

    session.exit(0)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!("Starting context example on 0.0.0.0:2222");
    tracing::info!("Connect with: ssh -p 2222 localhost");

    Server::new()
        .bind("0.0.0.0:2222")
        .pubkey_auth(|user, _key| async move {
            // A real server would look the key up; here we accept everyone and
            // derive an account from the username to attach to the session.
            let role = if user == "admin" { "admin" } else { "user" };
            Auth::accept().with(Account { id: 7, role })
        })
        .with(request_id)
        .app(app)
        .serve()
        .await
}
