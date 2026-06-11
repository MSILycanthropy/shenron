use crate::{Exit, Next, Session};

/// Middleware that rejects sessions without an active PTY.
///
/// # Errors
///
/// Returns `Err` if writing the rejection to the session fails.
pub async fn active_term(session: &mut Session, next: Next<'_>) -> crate::Result<Exit> {
    if session.pty().is_none() {
        session.write_stderr_str("PTY required\n").await?;

        return Ok(Exit::Code(1));
    }

    Ok(next.run(session).await)
}
