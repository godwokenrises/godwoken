// type aliases

use gw_types::U256;
pub use sparse_merkle_tree::H256;

pub trait H256Ext {
    fn one() -> H256;
    fn from_u32(n: u32) -> H256;
    fn to_u32(&self) -> u32;
    fn from_u64(n: u64) -> H256;
    fn to_u64(&self) -> u64;
    fn from_u128(n: u128) -> H256;
    fn to_u128(&self) -> u128;
    fn from_u256(n: U256) -> H256;
    fn to_u256(&self) -> U256;
}

impl H256Ext for H256 {
    fn one() -> H256 {
        H256::from_u32(1)
    }
    fn from_u32(n: u32) -> H256 {
        let mut buf = [0u8; 32];
        buf[..4].copy_from_slice(&n.to_le_bytes());
        buf.into()
    }
    fn to_u32(&self) -> u32 {
        let mut n_bytes = [0u8; 4];
        n_bytes.copy_from_slice(&self.as_slice()[..4]);
        u32::from_le_bytes(n_bytes)
    }
    fn from_u64(n: u64) -> H256 {
        let mut buf = [0u8; 32];
        buf[..8].copy_from_slice(&n.to_le_bytes());
        buf.into()
    }
    fn to_u64(&self) -> u64 {
        let mut n_bytes = [0u8; 8];
        n_bytes.copy_from_slice(&self.as_slice()[..8]);
        u64::from_le_bytes(n_bytes)
    }
    fn from_u128(n: u128) -> H256 {
        let mut buf = [0u8; 32];
        buf[..16].copy_from_slice(&n.to_le_bytes());
        buf.into()
    }
    fn to_u128(&self) -> u128 {
        let mut n_bytes = [0u8; 16];
        n_bytes.copy_from_slice(&self.as_slice()[..16]);
        u128::from_le_bytes(n_bytes)
    }
    fn from_u256(n: U256) -> H256 {
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
