use crate::blake2b::Blake2bHasher;
use sparse_merkle_tree::SparseMerkleTree;

// reexports
pub use sparse_merkle_tree::{default_store, error::Error, H256};

pub type SMT<S> = SparseMerkleTree<Blake2bHasher, H256, S>;
