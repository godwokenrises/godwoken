use core::cmp::Ordering;

use primitive_types::U256;

/// Represent 256 bits
#[derive(Eq, PartialEq, Debug, Default, Hash, Clone, Copy)]
pub struct H256([u8; 32]);

const ZERO: H256 = H256([0u8; 32]);

impl H256 {
    pub const fn zero() -> Self {
        ZERO
    }

    pub fn is_zero(&self) -> bool {
        self == &ZERO
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0[..]
    }

    pub fn one() -> H256 {
        H256::from_u32(1)
    }

    pub fn from_u32(n: u32) -> H256 {
        let mut buf = [0u8; 32];
        buf[..4].copy_from_slice(&n.to_le_bytes());
        buf.into()
    }

    pub fn to_u32(&self) -> u32 {
        let mut n_bytes = [0u8; 4];
        n_bytes.copy_from_slice(&self.as_slice()[..4]);
        u32::from_le_bytes(n_bytes)
    }

    pub fn from_u64(n: u64) -> H256 {
        let mut buf = [0u8; 32];
        buf[..8].copy_from_slice(&n.to_le_bytes());
        buf.into()
    }

    pub fn to_u64(&self) -> u64 {
        let mut n_bytes = [0u8; 8];
        n_bytes.copy_from_slice(&self.as_slice()[..8]);
        u64::from_le_bytes(n_bytes)
    }

    pub fn from_u128(n: u128) -> H256 {
        let mut buf = [0u8; 32];
        buf[..16].copy_from_slice(&n.to_le_bytes());
        buf.into()
    }

    pub fn to_u128(&self) -> u128 {
        let mut n_bytes = [0u8; 16];
        n_bytes.copy_from_slice(&self.as_slice()[..16]);
        u128::from_le_bytes(n_bytes)
    }

    pub fn from_u256(n: U256) -> H256 {
        let mut buf = [0u8; 32];
        n.to_little_endian(&mut buf);
        buf.into()
    }

    pub fn to_u256(&self) -> U256 {
        let mut n_bytes = [0u8; 32];
        n_bytes.copy_from_slice(&self.as_slice()[..32]);
        U256::from_little_endian(&n_bytes)
    }
}

impl PartialOrd for H256 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for H256 {
    fn cmp(&self, other: &Self) -> Ordering {
        // Compare bits from heigher to lower (255..0)
        self.0.iter().rev().cmp(other.0.iter().rev())
    }
}

impl From<[u8; 32]> for H256 {
    fn from(v: [u8; 32]) -> H256 {
        H256(v)
    }
}

impl From<H256> for [u8; 32] {
    fn from(h256: H256) -> [u8; 32] {
        h256.0
    }
}
