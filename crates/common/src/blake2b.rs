pub use blake2b_ref::{Blake2b, Blake2bBuilder};
use sparse_merkle_tree::{traits::Hasher, H256};

const BLAKE2B_KEY: &[u8] = &[];
const BLAKE2B_LEN: usize = 32;
const CKB_PERSONALIZATION: &[u8] = b"ckb-default-hash";

pub fn new_blake2b() -> Blake2b {
    Blake2bBuilder::new(32)
        .personal(CKB_PERSONALIZATION)
        .build()
}

pub struct Blake2bHasher(Blake2b);

impl Default for Blake2bHasher {
    fn default() -> Self {
        let blake2b = Blake2bBuilder::new(BLAKE2B_LEN)
            .personal(CKB_PERSONALIZATION)
            .key(BLAKE2B_KEY)
            .build();
        Blake2bHasher(blake2b)
    }
}

impl Hasher for Blake2bHasher {
    fn write_h256(&mut self, h: &H256) {
        self.0.update(h.as_slice());
    }
    fn finish(self) -> H256 {
        let mut hash = [0u8; 32];
        self.0.finalize(&mut hash);
        hash.into()
    }
}
