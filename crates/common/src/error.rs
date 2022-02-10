use crate::smt::Error as SMTError;

cfg_if::cfg_if! {
    if #[cfg(feature = "std")] {
        use thiserror::Error;
        #[derive(Error, Debug, Eq, PartialEq, Clone)]
        pub enum Error {
            #[error("{_0}")]
            SMT(SMTError),
            #[error("Amount overflow")]
            AmountOverflow,
            #[error("Merkle proof error")]
            MerkleProof,
            #[error("Missing key error")]
            MissingKey,
            #[error("Store error")]
            Store,
            #[error("Invalid short script hash error")]
            InvalidShortScriptHash,
            #[error("Duplicated script hash")]
            DuplicatedScriptHash,
        }
    } else {
        #[derive(Debug, Eq, PartialEq, Clone)]
        pub enum Error {
            SMT(SMTError),
            AmountOverflow,
            MerkleProof,
            MissingKey,
            Store,
            InvalidShortScriptHash,
            DuplicatedScriptHash,
        }
    }
}

impl From<SMTError> for Error {
    fn from(err: SMTError) -> Self {
        Error::SMT(err)
    }
}
