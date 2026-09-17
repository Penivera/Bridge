use std::fmt;

/// Error types encountered during proxy operations.
#[derive(Debug)]
pub enum ProxyError {
    /// An I/O error occurred during network operations or listener binding.
    Io(std::io::Error),
    /// A protocol error occurred within Hyper.
    Hyper(hyper::Error),
    /// A spawned task encountered a join failure.
    TaskJoin(tokio::task::JoinError),
}

impl fmt::Display for ProxyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Hyper(err) => write!(f, "Hyper protocol error: {err}"),
            Self::TaskJoin(err) => write!(f, "Task join error: {err}"),
        }
    }
}

impl std::error::Error for ProxyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Hyper(err) => Some(err),
            Self::TaskJoin(err) => Some(err),
        }
    }
}

impl From<std::io::Error> for ProxyError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<hyper::Error> for ProxyError {
    fn from(err: hyper::Error) -> Self {
        Self::Hyper(err)
    }
}

impl From<tokio::task::JoinError> for ProxyError {
    fn from(err: tokio::task::JoinError) -> Self {
        Self::TaskJoin(err)
    }
}

impl From<ProxyError> for std::io::Error {
    fn from(err: ProxyError) -> Self {
        match err {
            ProxyError::Io(err) => err,
            other => std::io::Error::other(other),
        }
    }
}

pub type Result<T> = std::result::Result<T, ProxyError>;
