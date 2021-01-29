use ckb_vm::Error as VMError;
use gw_common::{error::Error as StateError, sparse_merkle_tree::error::Error as SMTError};
use gw_types::packed::ChallengeTarget;
use thiserror::Error;

/// Error
#[derive(Error, PartialEq, Eq, Debug, Clone)]
pub enum Error {
    #[error("Transaction error {0}")]
    Transaction(TransactionErrorWithContext),
    #[error("State error {0:?}")]
    State(StateError),
    #[error("Validate error {0:?}")]
    Validate(ValidateError),
    #[error("Unlock error {0}")]
    Unlock(LockAlgorithmError),
    #[error("Deposition error {0}")]
    Deposition(DepositionError),
    #[error("Withdrawal error {0}")]
    Withdrawal(WithdrawalError),
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
    #[error("Unknown account lock")]
    UnknownAccountLock,
}

impl From<LockAlgorithmError> for Error {
    fn from(err: LockAlgorithmError) -> Self {
        Error::Unlock(err)
    }
}

#[derive(Error, Debug, PartialEq, Clone, Eq)]
pub enum DepositionError {
    #[error("Deposit Faked CKB")]
    DepositFakedCKB,
}

impl From<DepositionError> for Error {
    fn from(err: DepositionError) -> Self {
        Error::Deposition(err)
    }
}

#[derive(Error, Debug, PartialEq, Clone, Eq)]
pub enum WithdrawalError {
    #[error("Over withdrawal")]
    Overdraft,
    #[error("Invalid withdrawal nonce expected {expected} actual {actual}")]
    InvalidNonce { expected: u32, actual: u32 },
    #[error("Withdrawal Faked CKB")]
    WithdrawFakedCKB,
    #[error("Non positive sudt amount")]
    NonPositiveSUDTAmount,
}

impl From<WithdrawalError> for Error {
    fn from(err: WithdrawalError) -> Self {
        Error::Withdrawal(err)
    }
}

#[derive(Error, Debug, PartialEq, Clone, Eq)]
pub enum ValidateError {
    #[error("Insufficient capacity expected {expected} actual {actual}")]
    InsufficientCapacity { expected: u64, actual: u64 },
    #[error("Invalid SUDT operation")]
    InvalidSUDTOperation,
    #[error("Unknown SUDT")]
    UnknownSUDT,
    #[error("Unknown account")]
    UnknownAccount,
    #[error("Nonce Overflow")]
    NonceOverflow,
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
#[derive(Error, Debug, Clone, Eq, PartialEq)]
#[error("{error}")]
pub struct TransactionErrorWithContext {
    pub context: ChallengeTarget,
    pub error: TransactionError,
}

impl TransactionErrorWithContext {
    pub fn new(context: ChallengeTarget, error: TransactionError) -> Self {
        Self { context, error }
    }
}
