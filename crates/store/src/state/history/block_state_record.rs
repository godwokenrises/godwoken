use gw_types::h256::H256;
// block_number(8 bytes) | key (32 bytes)
#[derive(Hash, PartialEq, Eq)]
pub struct BlockStateRecordKey([u8; 40]);

impl BlockStateRecordKey {
    pub fn new(block_number: u64, state_key: &H256) -> Self {
        let mut inner = [0u8; 40];
        inner[..8].copy_from_slice(&block_number.to_be_bytes());
        inner[8..].copy_from_slice(state_key.as_slice());
        BlockStateRecordKey(inner)
    }

    pub fn state_key(&self) -> H256 {
        let mut inner = [0u8; 32];
        inner.copy_from_slice(&self.0[8..]);
        inner
    }

    pub fn block_number(&self) -> u64 {
        let mut inner = [0u8; 8];
        inner.copy_from_slice(&self.0[..8]);
        u64::from_be_bytes(inner)
    }

    pub fn from_slice(bytes: &[u8]) -> Self {
        let mut inner = [0u8; 40];
        inner.copy_from_slice(bytes);
        BlockStateRecordKey(inner)
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

//  key (32 bytes) | block_number(8 bytes)
pub(crate) struct BlockStateRecordKeyReverse([u8; 40]);

impl BlockStateRecordKeyReverse {
    pub fn new(block_number: u64, state_key: &H256) -> Self {
        let mut inner = [0u8; 40];
        inner[..32].copy_from_slice(state_key.as_slice());
        inner[32..].copy_from_slice(&block_number.to_be_bytes());
        BlockStateRecordKeyReverse(inner)
    }

    pub fn state_key(&self) -> H256 {
        let mut inner = [0u8; 32];
        inner.copy_from_slice(&self.0[..32]);
        inner
    }

    pub fn block_number(&self) -> u64 {
        let mut inner = [0u8; 8];
        inner.copy_from_slice(&self.0[32..]);
        u64::from_be_bytes(inner)
    }

    pub fn from_slice(bytes: &[u8]) -> Self {
        let mut inner = [0u8; 40];
        inner.copy_from_slice(bytes);
        BlockStateRecordKeyReverse(inner)
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}
