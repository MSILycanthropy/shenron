use std::path::Path;

use crate::{
    Middleware, Next, Result, Session, SessionKind,
    middleware::builtins::sftp::{
        filesystem::Filesystem, handler::SftpHandler, local::LocalFilesystem,
    },
};

/// Middleware that serves the `sftp` subsystem from a [`Filesystem`].
///
/// Non-SFTP sessions pass through to the next middleware untouched.
#[derive(Clone)]
pub struct Sftp<F: Filesystem> {
    fs: F,
}

impl<F: Filesystem> Sftp<F> {
    /// Serve SFTP requests from `fs`.
    pub const fn new(fs: F) -> Self {
        Self { fs }
    }
}

impl Sftp<LocalFilesystem> {
    /// Serve a real directory on disk, sandboxed to `root`.
    ///
    /// ```no_run
    /// use shenron::sftp::Sftp;
    ///
    /// let sftp = Sftp::local("/srv/files");
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if `root` cannot be opened as a directory. To handle the error,
    /// use [`Sftp::new`] with [`LocalFilesystem::try_new`].
    #[must_use]
    pub fn local(root: impl AsRef<Path>) -> Self {
        Self::new(LocalFilesystem::new(root))
    }
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
