use std::fmt::Write as _;

use tracing::{error, info};

use crate::{Exit, Next, Session, SessionKind};

/// Middleware that logs session starting, ending and errors
pub async fn logging(session: &mut Session, next: Next<'_>) -> Exit {
    let user = session.user().to_owned();
    let remote = session.remote_addr();
    let mut kind = match session.kind() {
        SessionKind::Exec { command } => format!("exec({command})"),
        SessionKind::Shell => "shell".to_string(),
        SessionKind::Subsystem { name } => format!("subsystem({name})"),
    };

    if let Some((term, size)) = session.pty() {
        let _ = write!(
            kind,
            " pty(term={}, size={}x{})",
            term, size.width, size.height
        );
    }

    info!(
        user = %user,
        remote = %remote,
        kind = %kind,
        "session started"
    );

    let start = std::time::Instant::now();
    let exit = next.run(session).await;
    let elapsed = start.elapsed();

    match &exit {
        Exit::Code(code) => {
            info!(
                user = %user,
                remote = %remote,
                elapsed = ?elapsed,
                exit_code = %code,
                "session ended"
            );
        }
        Exit::Error(e) => {
            error!(
                user = %user,
                remote = %remote,
                elapsed = ?elapsed,
                error = %e,
                "session error"
            );
        }
    }

    exit
}
