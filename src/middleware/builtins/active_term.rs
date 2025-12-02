use crate::{Next, Session};

/// Middleware that restricts
///
/// # Errors
///
/// Returns `err` if
///     The next middleware in the chain returns `Err`
///     Writing to the session fails
pub async fn active_term(session: Session, next: Next) -> crate::Result<Session> {
    if session.pty().is_none() {
        session.write_stderr_str("PTY required\n").await?;

        return session.abort(1).await;
    }

    next.run(session).await
}
