// type aliases

use gw_types::{core::H256, U256};
pub use sparse_merkle_tree::H256 as SMTH256;

pub trait SMTH256Ext {
    fn one() -> Self;
    fn to_h256(self) -> H256;
    fn from_h256(h: H256) -> Self;
    fn from_u32(n: u32) -> Self;
    fn to_u32(&self) -> u32;
    fn from_u64(n: u64) -> Self;
    fn to_u64(&self) -> u64;
    fn from_u128(n: u128) -> Self;
    fn to_u128(&self) -> u128;
    fn from_u256(n: U256) -> Self;
    fn to_u256(&self) -> U256;
}

impl SMTH256Ext for SMTH256 {
    fn one() -> Self {
        Self::from_u32(1)
    }

    fn to_h256(self) -> H256 {
        let b: [u8; 32] = self.into();
        b.into()
    }

    fn from_h256(h: H256) -> Self {
        let b: [u8; 32] = h.into();
        b.into()
    }

    fn from_u32(n: u32) -> Self {
        let mut buf = [0u8; 32];
        buf[..4].copy_from_slice(&n.to_le_bytes());
        buf.into()
    }

    fn to_u32(&self) -> u32 {
        let mut n_bytes = [0u8; 4];
        n_bytes.copy_from_slice(&self.as_slice()[..4]);
        u32::from_le_bytes(n_bytes)
    }

    fn from_u64(n: u64) -> Self {
        let mut buf = [0u8; 32];
        buf[..8].copy_from_slice(&n.to_le_bytes());
        buf.into()
    }

    fn to_u64(&self) -> u64 {
        let mut n_bytes = [0u8; 8];
        n_bytes.copy_from_slice(&self.as_slice()[..8]);
        u64::from_le_bytes(n_bytes)
    }

    fn from_u128(n: u128) -> Self {
        let mut buf = [0u8; 32];
        buf[..16].copy_from_slice(&n.to_le_bytes());
        buf.into()
    }

    fn to_u128(&self) -> u128 {
        let mut n_bytes = [0u8; 16];
        n_bytes.copy_from_slice(&self.as_slice()[..16]);
        u128::from_le_bytes(n_bytes)
    }

    fn from_u256(n: U256) -> Self {
        let mut buf = [0u8; 32];
        n.to_little_endian(&mut buf);
        buf.into()
    }

    fn to_u256(&self) -> U256 {
        let mut n_bytes = [0u8; 32];
        n_bytes.copy_from_slice(&self.as_slice()[..32]);
        U256::from_little_endian(&n_bytes)
    }
}

pub trait H256Ext {
    fn to_smt_h256(self) -> SMTH256;
    fn from_smt_h256(h: SMTH256) -> Self;
}

impl H256Ext for H256 {
    fn to_smt_h256(self) -> SMTH256 {
        let b: [u8; 32] = self.into();
        b.into()
    }

    fn from_smt_h256(h: SMTH256) -> Self {
        let b: [u8; 32] = h.into();
        b.into()
    }
}
