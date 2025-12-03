pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("SSH error: {0}")]
    Ssh(#[from] russh::Error),

    #[error("Key error: {0}")]
    Keys(#[from] russh::keys::Error),

    #[error("RuntimeError: {0}")]
    Io(#[from] std::io::Error),

    #[error("Protocol Error: {0}")]
    Protocol(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("RuntimeError: {0}")]
    Int(#[from] std::num::TryFromIntError),
}

impl From<Error> for std::io::Error {
    fn from(err: Error) -> Self {
        Self::other(err.to_string())
    }
}
