/// extension methods
use crate::packed::{L2Block, L2Transaction, RawL2Block, RawL2Transaction, Script};
use crate::prelude::*;
use core::mem::size_of_val;
use gw_hash::blake2b::new_blake2b;

impl RawL2Transaction {
    pub fn hash(&self) -> [u8; 32] {
        let mut hasher = new_blake2b();
        hasher.update(self.as_slice());
        let mut hash = [0u8; 32];
        hasher.finalize(&mut hash);
        hash
    }
}

impl RawL2Block {
    pub fn hash(&self) -> [u8; 32] {
        let mut hasher = new_blake2b();
        hasher.update(self.as_slice());
        let mut hash = [0u8; 32];
        hasher.finalize(&mut hash);
        hash
    }

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

impl L2Block {
    pub fn hash(&self) -> [u8; 32] {
        self.raw().hash()
    }

    pub fn smt_key(&self) -> [u8; 32] {
        self.raw().smt_key()
    }
}

impl L2Transaction {
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

impl Script {
    pub fn hash(&self) -> [u8; 32] {
        let mut hasher = new_blake2b();
        hasher.update(self.as_slice());
        let mut hash = [0u8; 32];
        hasher.finalize(&mut hash);
        hash
    }
}
