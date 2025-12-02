use async_trait::async_trait;

pub use crate::server::Server;
pub use crate::session::Session;

pub type Result<T> = std::result::Result<T, Error>;

pub mod server;
pub mod session;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("SSH error: {0}")]
    Ssh(#[from] russh::Error),

    #[error("Key error: {0}")]
    Keys(#[from] russh::keys::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Authentication failed")]
    AuthFailed,

    #[error("Session not found")]
    SessionNotFound,

    #[error("{0}")]
    Custom(String),
}

/// Trait for implementing SSH PTY applications
///
/// Implement this trait to define your SSH application's behavior.
/// The `handle` method is called for each authenticated PTY session.
///
/// # Example
///
/// ```rust
/// use shenron::{App, Session, Result};
/// use async_trait::async_trait;
///
/// struct MyApp;
///
/// #[async_trait]
/// impl App for MyApp {
///     async fn handle(&self, mut session: Session) -> Result<()> {
///         session.write_str("Hello, world!\r\n").await?;
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait App: Send + Sync + 'static {
    /// Handle an SSH PTY session
    ///
    /// This method is called when a client successfully authenticates
    /// and requests a PTY. Implement your application logic here.
    ///
    /// The session ends when this method returns.
    async fn handle(&self, session: Session) -> Result<()>;
}
