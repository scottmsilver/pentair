use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("buffer too short: need {need} bytes, have {have}")]
    BufferTooShort { need: usize, have: usize },

    #[error("invalid action code: {0}")]
    InvalidAction(u16),

    #[error("invalid data: {0}")]
    InvalidData(String),

    #[error("unknown variant: {kind} value {value}")]
    UnknownVariant { kind: &'static str, value: i32 },
}

pub type Result<T> = std::result::Result<T, ProtocolError>;
