use crate::ckb_std::error::SysError;
use gw_common::{error::Error as StateError, sparse_merkle_tree::error::Error as SMTError};

/// Error
#[repr(i8)]
pub enum Error {
    IndexOutOfBound = 1,
    ItemMissing,
    LengthNotEnough,
    Encoding,
    MerkleProof, // merkle verification error
    PrevGlobalState,
    PostGlobalState,
    AmountOverflow,
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

impl From<SMTError> for Error {
    fn from(_err: SMTError) -> Self {
        Error::MerkleProof
    }
}

impl From<StateError> for Error {
    fn from(err: StateError) -> Self {
        match err {
            StateError::AmountOverflow => Error::AmountOverflow,
            StateError::MerkleProof => Error::MerkleProof,
            StateError::SMT(_e) => Error::MerkleProof,
            StateError::MissingKey => Error::MerkleProof,
            StateError::Store => Error::MerkleProof,
        }
    }
}
