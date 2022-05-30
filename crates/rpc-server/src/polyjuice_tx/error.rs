use gw_common::{registry_address::RegistryAddress, H256};
use gw_types::{prelude::Pack, U256};

#[derive(thiserror::Error, Debug)]
pub enum PolyjuiceTxSenderRecoverError {
    #[error("mismatch chain id")]
    ChainId,
    #[error("to script not found")]
    ToScriptNotFound,
    #[error("invalid signature {0}")]
    InvalidSignature(anyhow::Error),
    #[error("{:x} insufficient ckb balance, expect {expect} got {got}", .registry_address.address.pack())]
    InsufficientCkbBalance {
        registry_address: RegistryAddress,
        expect: U256,
        got: U256,
    },
    #[error("{:x} is registered to script {:x}", .registry_address.address.pack(), .script_hash.pack())]
    DifferentScript {
        registry_address: RegistryAddress,
        script_hash: H256,
    },
    #[error("internal {0}")]
    Internal(#[from] anyhow::Error),
}

impl From<gw_common::error::Error> for PolyjuiceTxSenderRecoverError {
    fn from(err: gw_common::error::Error) -> Self {
        Self::Internal(err.into())
    }
}
