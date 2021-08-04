/// extension methods
use crate::packed;
use crate::prelude::*;
use core::mem::size_of_val;
use gw_hash::blake2b::new_blake2b;

macro_rules! impl_hash {
    ($struct:ident) => {
        impl<'a> packed::$struct<'a> {
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
        impl<'a> packed::$struct<'a> {
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

impl_hash!(RollupConfigReader);
impl_hash!(ScriptReader);
impl_hash!(RawL2BlockReader);
impl_hash!(RawL2TransactionReader);
impl_witness_hash!(L2TransactionReader);
impl_hash!(RawWithdrawalRequestReader);
impl_witness_hash!(WithdrawalRequestReader);
impl_hash!(RawTransactionReader);
impl_witness_hash!(TransactionReader);
impl_hash!(HeaderReader);

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

impl packed::RawL2Transaction {
    pub fn hash(&self) -> [u8; 32] {
        self.as_reader().hash()
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

impl packed::Script {
    pub fn hash(&self) -> [u8; 32] {
        self.as_reader().hash()
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

impl packed::Header {
    pub fn hash(&self) -> [u8; 32] {
        self.as_reader().hash()
    }
}

impl packed::Transaction {
    pub fn hash(&self) -> [u8; 32] {
        self.as_reader().hash()
    }
}

impl packed::RollupConfig {
    pub fn hash(&self) -> [u8; 32] {
        self.as_reader().hash()
    }
}

impl packed::CellOutput {
    pub fn occupied_capacity(&self, data_capacity: usize) -> ckb_types::core::CapacityResult<u64> {
        let output = ckb_types::packed::CellOutput::new_unchecked(self.as_bytes());
        output
            .occupied_capacity(ckb_types::core::Capacity::bytes(data_capacity)?)
            .map(|c| c.as_u64())
    }
}
