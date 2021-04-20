/// extension methods
use crate::packed;
use crate::prelude::*;
use core::mem::size_of_val;
use gw_hash::blake2b::new_blake2b;

macro_rules! impl_hash {
    ($struct:ident) => {
        impl packed::$struct {
            pub fn hash(&self) -> [u8; 32] {
                let mut hasher = new_blake2b();
                hasher.update(self.as_slice());
                let mut hash = [0u8; 32];
                hasher.finalize(&mut hash);
                hash
            }
        }
    };
}

macro_rules! impl_witness_hash {
    ($struct:ident) => {
        impl packed::$struct {
            pub fn hash(&self) -> [u8; 32] {
                self.raw().hash()
            }

            pub fn witness_hash(&self) -> [u8; 32] {
                let mut hasher = new_blake2b();
                hasher.update(self.as_slice());
                let mut hash = [0u8; 32];
                hasher.finalize(&mut hash);
                hash
            }
        }
    };
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
}

impl packed::L2Block {
    pub fn hash(&self) -> [u8; 32] {
        self.raw().hash()
    }

    pub fn smt_key(&self) -> [u8; 32] {
        self.raw().smt_key()
    }
}

impl_hash!(RollupConfig);
impl_hash!(Script);
impl_hash!(RawL2Block);
impl_hash!(RawL2Transaction);
impl_witness_hash!(L2Transaction);
impl_hash!(RawWithdrawalRequest);
impl_witness_hash!(WithdrawalRequest);
impl_hash!(RawTransaction);
impl_witness_hash!(Transaction);
impl_hash!(RawHeader);

cfg_if::cfg_if! {
    if #[cfg(feature = "std")] {
        impl packed::TransactionKey {
            pub fn build_transaction_key(block_hash: crate::packed::Byte32, index: u32) -> Self {
                let mut key = [0u8; 36];
                key[..32].copy_from_slice(block_hash.as_slice());
                // use BE, so we have a sorted bytes representation
                key[32..].copy_from_slice(&index.to_be_bytes());
                key.pack()
            }
        }
    }
}
