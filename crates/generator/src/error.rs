use ckb_vm::Error as VMError;
use gw_common::{error::Error as StateError, sparse_merkle_tree::error::Error as SMTError};
use gw_types::{packed::StartChallenge, prelude::*};
use thiserror::Error;

/// Error
#[derive(Error, Debug, Clone)]
pub enum Error {
    #[error("Transaction error {0}")]
    Transaction(TransactionErrorWithContext),
    #[error("State error {0:?}")]
    State(StateError),
    #[error("Validate error {0:?}")]
    Validate(ValidateError),
}

impl From<StateError> for Error {
    fn from(err: StateError) -> Self {
        Error::State(err)
    }
}

#[derive(Error, Debug, PartialEq, Clone, Eq)]
pub enum LockAlgorithmError {
    #[error("Invalid lock args")]
    InvalidLockArgs,
    #[error("Invalid signature")]
    InvalidSignature,
}

impl From<LockAlgorithmError> for ValidateError {
    fn from(err: LockAlgorithmError) -> Self {
        ValidateError::Unlock(err)
    }
}

impl From<LockAlgorithmError> for Error {
    fn from(err: LockAlgorithmError) -> Self {
        ValidateError::Unlock(err).into()
    }
}

#[derive(Error, Debug, PartialEq, Clone, Eq)]
pub enum ValidateError {
    #[error("Invalid withdrawal request")]
    InvalidWithdrawal,
    #[error("Invalid withdrawal nonce expected {expected} actual {actual}")]
    InvalidWithdrawalNonce { expected: u32, actual: u32 },
    #[error("Unknown account lock script")]
    UnknownAccountLockScript,
    #[error("Unlock error {0}")]
    Unlock(LockAlgorithmError),
    #[error("Insufficient capacity expected {expected} actual {actual}")]
    InsufficientCapacity { expected: u64, actual: u64 },
    #[error("Invalid SUDT operation")]
    InvalidSUDTOperation,
}

impl From<ValidateError> for Error {
    fn from(err: ValidateError) -> Self {
        Error::Validate(err)
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
    #[error("Unknown backend account_id {account_id}")]
    Backend { account_id: u32 },
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
    pub context: StartChallenge,
    pub error: TransactionError,
}

impl PartialEq for TransactionErrorWithContext {
    fn eq(&self, other: &Self) -> bool {
        self.context.as_slice() == other.context.as_slice() && self.error == other.error
    }
}

impl Eq for TransactionErrorWithContext {}

impl TransactionErrorWithContext {
    pub fn new(context: StartChallenge, error: TransactionError) -> Self {
        Self { context, error }
    }
}
