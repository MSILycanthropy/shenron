use crate::{Exit, Next, Session};

/// Middleware that prints the elapsed time the session took
///
/// # Errors
///
/// Returns `Err` if writing to the session fails.
pub async fn elapsed(session: &mut Session, next: Next<'_>) -> crate::Result<Exit> {
    let start = std::time::Instant::now();
    let exit = next.run(session).await;

    session
        .write_str(&format!("Session lasted: {:?}\r\n", start.elapsed()))
        .await?;

    Ok(exit)
}
