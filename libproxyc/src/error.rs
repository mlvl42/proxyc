use thiserror::Error as ThisError;

#[derive(ThisError, Debug)]
pub enum Error {
    #[error("poll timeout")]
    Timeout,
    #[error("socket error")]
    SocketError,
    #[error("connect error: {0}")]
    ConnectError(String),
    #[error("missing data")]
    MissingData,
    #[error("{0}")]
    Generic(String),
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    Errno(#[from] nix::errno::Errno),
}
