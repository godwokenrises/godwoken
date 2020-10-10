use ckb_vm::Error as VMError;
use sparse_merkle_tree::error::Error as SMTError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq, Clone, Eq)]
pub enum Error {
    #[error("invalid exit code {0}")]
    InvalidExitCode(i8),
    #[error("VM error {0}")]
    VM(VMError),
    #[error("SMT error {0}")]
    SMT(SMTError),
    #[error("invalid nonce expected {expected}, actual {actual}")]
    Nonce { expected: u32, actual: u32 },
}

impl From<VMError> for Error {
    fn from(err: VMError) -> Self {
        Error::VM(err)
    }
}

impl From<SMTError> for Error {
    fn from(err: SMTError) -> Self {
        Error::SMT(err)
    }
}
