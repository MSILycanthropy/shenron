use crate::{
    Middleware, Next, Result, Session, SessionKind,
    middleware::builtins::sftp::{filesystem::Filesystem, handler::SftpHandler},
};

#[derive(Clone)]
pub struct Sftp<F: Filesystem> {
    fs: F,
}

impl<F: Filesystem> Middleware for Sftp<F> {
    async fn handle(&self, mut session: Session, next: Next) -> Result<Session> {
        match session.kind() {
            SessionKind::Subsystem { name } if name == "sftp" => {
                let stream = session.unsafe_take_channel().into_stream();
                let handler = SftpHandler::new(self.fs.clone());

                russh_sftp::server::run(stream, handler).await;

                Ok(session)
            }
            _ => next.run(session).await,
        }
    }
}
