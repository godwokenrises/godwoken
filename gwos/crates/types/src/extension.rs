use core::mem::size_of_val;

use gw_hash::blake2b::hash;

/// extension methods
use crate::packed;
use crate::prelude::*;

macro_rules! impl_hash {
    ($struct:ident) => {
        impl<'a> packed::$struct<'a> {
            pub fn hash(&self) -> [u8; 32] {
                hash(self.as_slice())
            }
        }
    };
}

macro_rules! impl_witness_hash {
    ($struct:ident) => {
        impl<'a> packed::$struct<'a> {
            pub fn hash(&self) -> [u8; 32] {
                self.raw().hash()
            }

            pub fn witness_hash(&self) -> [u8; 32] {
                hash(self.as_slice())
            }
        }
    };
}

impl_hash!(RollupConfigReader);
impl_hash!(RawL2BlockReader);
impl_hash!(RawL2TransactionReader);
impl_witness_hash!(L2TransactionReader);
impl_hash!(RawWithdrawalRequestReader);
impl_witness_hash!(WithdrawalRequestReader);

impl packed::RawL2Transaction {
    pub fn hash(&self) -> [u8; 32] {
        self.as_reader().hash()
    }

    pub fn is_chain_id_protected(&self) -> bool {
        self.chain_id().unpack() != 0
    }
}

impl packed::L2Transaction {
    pub fn hash(&self) -> [u8; 32] {
        self.raw().hash()
    }

    pub fn witness_hash(&self) -> [u8; 32] {
        self.as_reader().witness_hash()
    }
}

impl packed::RawL2Block {
    pub fn smt_key(&self) -> [u8; 32] {
        Self::compute_smt_key(self.number().unpack())
    }

    // Block SMT key
    pub fn compute_smt_key(block_number: u64) -> [u8; 32] {
        let mut buf = [0u8; 32];
        buf[..size_of_val(&block_number)].copy_from_slice(&block_number.to_le_bytes());
        buf
    }

    pub fn hash(&self) -> [u8; 32] {
        self.as_reader().hash()
    }
}

impl packed::L2Block {
    pub fn hash(&self) -> [u8; 32] {
        self.raw().hash()
    }

    pub fn smt_key(&self) -> [u8; 32] {
        self.raw().smt_key()
    }
}

impl packed::RawWithdrawalRequest {
    pub fn hash(&self) -> [u8; 32] {
        self.as_reader().hash()
    }
}

impl packed::WithdrawalRequest {
    pub fn hash(&self) -> [u8; 32] {
        self.raw().hash()
    }

    pub fn witness_hash(&self) -> [u8; 32] {
        self.as_reader().witness_hash()
    }
}

impl packed::RollupConfig {
    pub fn hash(&self) -> [u8; 32] {
        self.as_reader().hash()
    }
}
