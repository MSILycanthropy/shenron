pub type Result<T = ()> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("SSH error: {0}")]
    Ssh(#[from] russh::Error),

    #[error("Key error: {0}")]
    Keys(#[from] russh::keys::Error),

    #[error("SSH key error: {0}")]
    SshKey(#[from] russh::keys::ssh_key::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Handler panicked: {0}")]
    Panic(String),

    #[error("Integer conversion error: {0}")]
    Int(#[from] std::num::TryFromIntError),
}

impl From<Error> for std::io::Error {
    fn from(err: Error) -> Self {
        Self::other(err.to_string())
    }
}
