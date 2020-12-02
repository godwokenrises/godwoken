use ckb_vm::Error as VMError;
use gw_common::{sparse_merkle_tree::error::Error as SMTError, state::Error as StateError};
use gw_types::{packed::StartChallenge, prelude::*};

use thiserror::Error;

/// Error
#[derive(Error, Debug, Clone)]
pub enum Error {
    #[error("Transaction error {0}")]
    Transaction(TransactionErrorWithContext),
    #[error("State error {0:?}")]
    State(StateError),
}

impl From<StateError> for Error {
    fn from(err: StateError) -> Self {
        Error::State(err)
    }
}

impl From<TransactionErrorWithContext> for Error {
    fn from(err: TransactionErrorWithContext) -> Self {
        Error::Transaction(err)
    }
}

/// Transaction error
#[derive(Error, Debug, PartialEq, Clone, Eq)]
pub enum TransactionError {
    #[error("invalid exit code {0}")]
    InvalidExitCode(i8),
    #[error("VM error {0}")]
    VM(VMError),
    #[error("SMT error {0}")]
    SMT(SMTError),
    #[error("invalid nonce expected {expected}, actual {actual}")]
    Nonce { expected: u32, actual: u32 },
    #[error("State error {0:?}")]
    State(StateError),
}

impl From<VMError> for TransactionError {
    fn from(err: VMError) -> Self {
        TransactionError::VM(err)
    }
}

impl From<SMTError> for TransactionError {
    fn from(err: SMTError) -> Self {
        TransactionError::SMT(err)
    }
}

impl From<StateError> for TransactionError {
    fn from(err: StateError) -> Self {
        TransactionError::State(err)
    }
}

/// Transaction error with challenge context
#[derive(Error, Debug, Clone)]
#[error("{error}")]
pub struct TransactionErrorWithContext {
    pub challenge_context: StartChallenge,
    pub error: TransactionError,
}

impl PartialEq for TransactionErrorWithContext {
    fn eq(&self, other: &Self) -> bool {
        self.challenge_context.as_slice() == other.challenge_context.as_slice()
            && self.error == other.error
    }
}

impl Eq for TransactionErrorWithContext {}

impl TransactionErrorWithContext {
    pub fn new(challenge_context: StartChallenge, error: TransactionError) -> Self {
        Self {
            challenge_context,
            error,
        }
    }
}
