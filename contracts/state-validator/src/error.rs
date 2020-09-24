use ckb_std::error::SysError;

/// Error
#[repr(i8)]
pub enum Error {
    IndexOutOfBound = 1,
    ItemMissing,
    LengthNotEnough,
    Encoding,
    SubmitInvalidBlock,
    WrongSignature,
    MerkleProof, // merkle verification error
    PrevGlobalState,
    PostGlobalState,
    SUDT, // invalid SUDT
    Secp256k1, // secp256k1 error
    KVMissing, // missing KV pair
    UnexpectedRollupLock,
    DepositionValue, // incorrect deposition value
    AmountOverflow,
    InvalidTxs,
    Aggregator,
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

