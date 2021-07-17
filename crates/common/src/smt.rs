use gw_hash::blake2b::{new_blake2b, Blake2b};
use sparse_merkle_tree::{traits::Hasher, SparseMerkleTree};

// re-exports
pub use sparse_merkle_tree::{
    default_store, error::Error, traits::Store, CompiledMerkleProof, MerkleProof, H256,
};

pub type SMT<S> = SparseMerkleTree<Blake2bHasher, H256, S>;

pub struct Blake2bHasher(Blake2b);

impl Default for Blake2bHasher {
    fn default() -> Self {
        Blake2bHasher(new_blake2b())
    }
}

impl Hasher for Blake2bHasher {
    fn write_h256(&mut self, h: &H256) {
        self.0.update(h.as_slice());
    }

    fn write_byte(&mut self, b: u8) {
        self.0.update(&[b][..]);
    }

    fn finish(self) -> H256 {
        let mut hash = [0u8; 32];
        self.0.finalize(&mut hash);
        hash.into()
    }
}
