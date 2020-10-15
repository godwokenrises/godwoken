use ckb_vm::Error as VMError;
use gw_common::state::Error as StateError;
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
    #[error("State error {0:?}")]
    State(StateError),
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

impl From<StateError> for Error {
    fn from(err: StateError) -> Self {
        Error::State(err)
    }
}
