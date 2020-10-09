use ckb_vm::Error as VMError;
pub use godwoken_types::bytes;
use thiserror::Error;

#[derive(Error, Debug, PartialEq, Clone, Eq)]
pub enum Error {
    #[error("invalid exit code {0}")]
    InvalidExitCode(i8),
    #[error("VM error {0}")]
    VM(VMError),
}

impl From<VMError> for Error {
    fn from(err: VMError) -> Self {
        Error::VM(err)
    }
}
