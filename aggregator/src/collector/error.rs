use thiserror::Error;

#[derive(Error, Debug, PartialEq, Clone, Eq)]
pub enum Error {
    #[error("RPC error {0}")]
    RPC(String),
}
