use crate::blake2b::Blake2bHasher;
use sparse_merkle_tree::SparseMerkleTree;

// re-exports
pub use sparse_merkle_tree::{default_store, error::Error, traits::Store, H256};

pub type SMT<S> = SparseMerkleTree<Blake2bHasher, H256, S>;
