use crate::{Next, Session};

/// Middleware that prints the elapsed time the session took
///
/// # Errors
///
/// Returns `err` if
///     The next middleware in the chain returns `Err`
///     Writing to the session fails
pub async fn elapsed(session: Session, next: Next) -> crate::Result<Session> {
    let start = std::time::Instant::now();
    let session = next.run(session).await?;
    session
        .write_str(&format!("Session lasted: {:?}\r\n", start.elapsed()))
        .await?;
    Ok(session)
}
