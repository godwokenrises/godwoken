use crate::blake2b::Blake2bHasher;
use crate::state::{Error as StateError, State};
use sparse_merkle_tree::SparseMerkleTree;

// re-exports
pub use sparse_merkle_tree::{default_store, error::Error, traits::Store, H256};

pub type SMT<S> = SparseMerkleTree<Blake2bHasher, H256, S>;

impl<S: Store<H256>> State for SMT<S> {
    fn get_raw(&self, key: &[u8; 32]) -> Result<[u8; 32], StateError> {
        let v = self.get(&(*key).into())?;
        Ok(v.into())
    }

    fn update_raw(&mut self, key: [u8; 32], value: [u8; 32]) -> Result<(), StateError> {
        self.update(key.into(), value.into())?;
        Ok(())
    }

    fn calculate_root(&self) -> Result<[u8; 32], StateError> {
        Ok((*self.root()).into())
    }
}
