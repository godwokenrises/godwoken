//! godwoken validator errors

use ckb_std::error::SysError;
use gw_common::{error::Error as CommonError, smt::Error as SMTError};

/// Error
#[repr(i8)]
pub enum Error {
    IndexOutOfBound = 1,
    ItemMissing = 2,
    LengthNotEnough = 3,
    Encoding = 4,
    // Add customized errors here...
    InvalidArgs = 5,
    InvalidSince = 6,
    InvalidOutput = 7,
    OwnerCellNotFound = 8,
    RollupCellNotFound = 9,
    RollupConfigNotFound = 10,
    ProofNotFound = 11,
    AccountNotFound = 12,
    MerkleProof = 13,
    AmountOverflow = 14,
    InsufficientAmount = 15,
    InsufficientInputFinalizedAssets = 16,
    InsufficientOutputFinalizedAssets = 17,
    SMTKeyMissing = 18,
    InvalidStateCheckpoint = 19,
    InvalidBlock = 20,
    InvalidStatus = 21,
    InvalidStakeCellUnlock = 22,
    InvalidPostGlobalState = 23,
    InvalidChallengeCell = 24,
    InvalidStakeCell = 25,
    InvalidDepositCell = 26,
    InvalidWithdrawalCell = 27,
    InvalidCustodianCell = 28,
    InvalidRevertedBlocks = 29,
    InvalidChallengeReward = 30,
    InvalidSUDTCell = 31,
    InvalidChallengeTarget = 32,
    InvalidWithdrawalRequest = 33,
    UnknownEOAScript = 34,
    UnknownContractScript = 35,
    ScriptNotFound = 36,
    AccountLockCellNotFound = 37,
    AccountScriptCellNotFound = 38,
    InvalidTypeID = 39,
    UnexpectedTxNonce = 40,
    // raise from signature verification script
    WrongSignature = 41,
    DuplicatedScriptHash = 42,
    RegistryAddressNotFound = 43,
    DuplicatedRegistryAddress = 44,
    NotFinalized = 45,
}

impl From<SysError> for Error {
    fn from(err: SysError) -> Self {
        use SysError::*;
        match err {
            IndexOutOfBound => Self::IndexOutOfBound,
            ItemMissing => Self::ItemMissing,
            LengthNotEnough(_) => Self::LengthNotEnough,
            Encoding => Self::Encoding,
            Unknown(err_code) => panic!("unexpected sys error {}", err_code),
        }
    }
}

impl From<CommonError> for Error {
    fn from(err: CommonError) -> Self {
        use CommonError::*;
        match err {
            SMT(_) | Store | MissingKey => Self::SMTKeyMissing,
            MerkleProof => Self::MerkleProof,
            AmountOverflow => Self::AmountOverflow,
            DuplicatedScriptHash => Self::DuplicatedScriptHash,
            InvalidArgs => Self::InvalidArgs,
            UnknownEoaCodeHash => Self::UnknownEOAScript,
            DuplicatedRegistryAddress => Self::DuplicatedRegistryAddress,
        }
    }
}

impl From<SMTError> for Error {
    fn from(_err: SMTError) -> Self {
        Self::SMTKeyMissing
    }
}
