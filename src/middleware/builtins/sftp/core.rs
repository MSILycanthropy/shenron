use crate::{
    Middleware, Next, Result, Session, SessionKind,
    middleware::builtins::sftp::{filesystem::Filesystem, handler::SftpHandler},
};

#[derive(Clone)]
pub struct Sftp<F: Filesystem> {
    fs: F,
}

impl<F: Filesystem> Middleware for Sftp<F> {
    async fn handle(&self, session: &'_ mut Session, next: Next<'_>) -> Result {
        match session.kind() {
            SessionKind::Subsystem { name } if name == "sftp" => {
                let Some(channel) = session.take_channel() else {
                    return Ok(());
                };

                let stream = channel.into_stream();
                let handler = SftpHandler::new(self.fs.clone());

                russh_sftp::server::run(stream, handler).await;

                Ok(())
            }
            _ => next.run(session).await,
        }
    }
}
