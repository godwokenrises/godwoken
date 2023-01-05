use thiserror::Error;

#[derive(Error, Debug)]
pub enum RpcClientError {
    #[error("connection failed by: {0}, error: {1}")]
    ConnectionError(String, anyhow::Error),

    #[error(transparent)]
    SerdeJsonError(#[from] serde_json::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
