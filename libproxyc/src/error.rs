use thiserror::Error as ThisError;

#[derive(ThisError, Debug)]
pub enum Error {
    #[error("poll timeout")]
    Timeout,
    #[error("socket error")]
    Socket,
    #[error("connect error: {0}")]
    Connect(String),
    #[error("missing data")]
    MissingData,
    #[error("{0}")]
    Generic(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Errno(#[from] nix::errno::Errno),
}
