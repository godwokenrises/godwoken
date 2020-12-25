use thiserror::Error;

/// DB Error
#[derive(Error, Debug, Clone)]
#[error("DB error {message}")]
pub struct Error {
    pub message: String,
}

impl From<String> for Error {
    fn from(msg: String) -> Self {
        Error { message: msg }
    }
}
