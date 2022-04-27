use ckb_vm::Error as VMError;
use gw_common::{error::Error as StateError, sparse_merkle_tree::error::Error as SMTError, H256};
use gw_types::packed::Byte32;
use thiserror::Error;

/// Error
#[derive(Error, PartialEq, Eq, Debug, Clone)]
pub enum Error {
    #[error("Transaction error {0}")]
    Transaction(TransactionError),
    #[error("State error {0:?}")]
    State(StateError),
    #[error("Account error {0:?}")]
    Account(AccountError),
    #[error("Unlock error {0}")]
    Unlock(LockAlgorithmError),
    #[error("Deposit error {0}")]
    Deposit(DepositError),
    #[error("Withdrawal error {0}")]
    Withdrawal(WithdrawalError),
    #[error("Block error {0}")]
    Block(BlockError),
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
    #[error("Invalid transaction args")]
    InvalidTransactionArgs,
}

impl From<LockAlgorithmError> for Error {
    fn from(err: LockAlgorithmError) -> Self {
        Error::Unlock(err)
    }
}

#[derive(Error, Debug, PartialEq, Clone, Eq)]
pub enum DepositError {
    #[error("Deposit faked CKB")]
    DepositFakedCKB,
    #[error("Deposit unknown EoA lock")]
    DepositUnknownEoALock,
}

impl From<DepositError> for Error {
    fn from(err: DepositError) -> Self {
        Error::Deposit(err)
    }
}

#[derive(Error, Debug, PartialEq, Clone, Eq)]
pub enum WithdrawalError {
    #[error("Over withdrawal")]
    Overdraft,
    #[error("Invalid withdrawal nonce expected {expected} actual {actual}")]
    Nonce { expected: u32, actual: u32 },
    #[error("Withdrawal Faked CKB")]
    WithdrawFakedCKB,
    #[error("Non positive sudt amount")]
    NonPositiveSUDTAmount,
    #[error("Expected owner lock hash {0}")]
    OwnerLock(Byte32),
    #[error("Expected v1 deposit lock hash {0}")]
    V1DepositLock(Byte32),
}

impl From<WithdrawalError> for Error {
    fn from(err: WithdrawalError) -> Self {
        Error::Withdrawal(err)
    }
}

#[derive(Error, Debug, PartialEq, Clone, Eq)]
pub enum AccountError {
    #[error("Insufficient capacity expected {expected} actual {actual}")]
    InsufficientCapacity { expected: u128, actual: u64 },
    #[error("Invalid SUDT operation")]
    InvalidSUDTOperation,
    #[error("Unknown SUDT")]
    UnknownSUDT,
    #[error("Unknown account")]
    UnknownAccount,
    #[error("Unknown script")]
    UnknownScript,
    #[error("Nonce Overflow")]
    NonceOverflow,
    #[error("can't find script for account {account_id}")]
    ScriptNotFound { account_id: u32 },
}

impl From<AccountError> for Error {
    fn from(err: AccountError) -> Self {
        Error::Account(err)
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
    #[error("invalid nonce of account {account_id} expected {expected}, actual {actual}")]
    Nonce {
        account_id: u32,
        expected: u32,
        actual: u32,
    },
    #[error("State error {0:?}")]
    State(StateError),
    #[error("can't find backend for script_hash {script_hash:?}")]
    BackendNotFound { script_hash: H256 },
    #[error("Exceeded maximum read data: max bytes {max_bytes}, readed bytes {used_bytes}")]
    ExceededMaxReadData { max_bytes: usize, used_bytes: usize },
    #[error("Exceeded maximum write data: max bytes {max_bytes}, writen bytes {used_bytes}")]
    ExceededMaxWriteData { max_bytes: usize, used_bytes: usize },
    #[error("Cannot create sUDT proxy contract from account id: {account_id}.")]
    InvalidSUDTProxyCreatorAccount { account_id: u32 },
    #[error("Cannot create backend {} contract from account id: {account_id}")]
    InvalidContractCreatorAccount {
        backend: &'static str,
        account_id: u32,
    },
    #[error("Backend must update nonce")]
    BackendMustIncreaseNonce,
    #[error("ScriptHashNotFound")]
    ScriptHashNotFound,
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
pub enum TransactionValidateError {
    #[error("Transaction error {0}")]
    Transaction(TransactionError),
    #[error("State error {0:?}")]
    State(StateError),
    #[error("Account error {0:?}")]
    Account(AccountError),
    #[error("Unlock error {0}")]
    Unlock(LockAlgorithmError),
}

impl From<TransactionError> for TransactionValidateError {
    fn from(err: TransactionError) -> Self {
        Self::Transaction(err)
    }
}

impl From<AccountError> for TransactionValidateError {
    fn from(err: AccountError) -> Self {
        Self::Account(err)
    }
}

impl From<LockAlgorithmError> for TransactionValidateError {
    fn from(err: LockAlgorithmError) -> Self {
        Self::Unlock(err)
    }
}

impl From<StateError> for TransactionValidateError {
    fn from(err: StateError) -> Self {
        Self::State(err)
    }
}

impl From<TransactionValidateError> for Error {
    fn from(err: TransactionValidateError) -> Self {
        match err {
            TransactionValidateError::Transaction(err) => Error::Transaction(err),
            TransactionValidateError::State(err) => Error::State(err),
            TransactionValidateError::Account(err) => Error::Account(err),
            TransactionValidateError::Unlock(err) => Error::Unlock(err),
        }
    }
}

#[derive(Error, Debug, PartialEq, Clone, Eq)]
pub enum BlockError {
    #[error("Invalid checkpoint at {index}, expected: {expected_checkpoint:?}, block: {block_checkpoint:?}")]
    InvalidCheckpoint {
        expected_checkpoint: H256,
        block_checkpoint: H256,
        index: usize,
    },
    #[error("Can't find checkpoint at index {index}")]
    CheckpointNotFound { index: usize },
}

impl From<BlockError> for Error {
    fn from(err: BlockError) -> Self {
        Error::Block(err)
    }
}
