use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("protocol error: {0}")]
    Protocol(#[from] pentair_protocol::error::ProtocolError),

    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    #[error("not connected")]
    NotConnected,

    #[error("login failed")]
    LoginFailed,

    #[error("timeout")]
    Timeout,

    #[error("discovery failed: no adapters found")]
    DiscoveryFailed,

    #[error("write rejected: {0}")]
    WriteRejected(String),
}

pub type Result<T> = std::result::Result<T, ClientError>;
