use tracing::{error, info};

use crate::{Next, Session, SessionKind};

/// Middleware that logs session starting, ending and erors
///
/// # Errors
///
/// Returns `err` if
///   - The next middleware in the chain returns `Err`
pub async fn logging(session: Session, next: Next) -> crate::Result<Session> {
    let user = session.user().to_owned();
    let remote = session.remote_addr();
    let kind = match session.kind() {
        SessionKind::Pty { term, size } => {
            format!("pty(term={}, size={}x{})", term, size.width, size.height)
        }
        SessionKind::Exec { command } => format!("exec({command})"),
        SessionKind::Shell => "shell".to_string(),
        SessionKind::Subsystem { name } => format!("subsystem({name})"),
    };

    info!(
        user = %user,
        remote = %remote,
        kind = %kind,
        "session started"
    );

    let start = std::time::Instant::now();
    let result = next.run(session).await;
    let elapsed = start.elapsed();

    match &result {
        Ok(session) => {
            let exit_code = session.exit_code().unwrap_or(0);
            info!(
                user = %user,
                remote = %remote,
                elapsed = ?elapsed,
                exit_code = %exit_code,
                "session ended"
            );
        }
        Err(e) => {
            error!(
                user = %user,
                remote = %remote,
                elapsed = ?elapsed,
                error = %e,
                "session error"
            );
        }
    }

    result
}
