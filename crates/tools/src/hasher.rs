use ckb_fixed_hash::H256;
use gw_common::blake2b::{new_blake2b, Blake2b};
use sha3::{Digest, Keccak256};

pub struct CkbHasher {
    hasher: Blake2b,
}

impl CkbHasher {
    pub fn new() -> Self {
        Self {
            hasher: new_blake2b(),
        }
    }

    pub fn update(mut self, data: &[u8]) -> Self {
        self.hasher.update(data);
        self
    }

    pub fn finalize(self) -> H256 {
        let mut hash = [0u8; 32];
        self.hasher.finalize(&mut hash);
        hash.into()
    }
}

pub struct EthHasher {
    hasher: Keccak256,
}

impl EthHasher {
    pub fn new() -> Self {
        Self {
            hasher: Keccak256::new(),
        }
    }

    pub fn update(mut self, data: impl AsRef<[u8]>) -> Self {
        self.hasher.update(data);
        self
    }

    pub fn finalize(self) -> H256 {
        let buf = self.hasher.finalize();
        let result: [u8; 32] = buf.into();
        result.into()
    }
}
