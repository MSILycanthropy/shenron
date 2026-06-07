use crate::{Next, Session};

/// Middleware that rejects sessions without an active PTY.
///
/// # Errors
///
/// Returns `Err` if
///   - The next middleware in the chain returns `Err`
///   - Writing to the session fails
pub async fn active_term(session: Session, next: Next) -> crate::Result<Session> {
    if session.pty().is_none() {
        session.write_stderr_str("PTY required\n").await?;

        return session.exit(1);
    }

    next.run(session).await
}
