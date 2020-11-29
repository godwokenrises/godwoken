use crate::vec::Vec;
use crate::{
    blake2b::new_blake2b,
    h256_ext::H256Ext,
    smt::{default_store::DefaultStore, Error, H256, SMT},
};

// Calculate compacted account root
pub fn calculate_compacted_account_root(root: &[u8], count: u32) -> [u8; 32] {
    let mut buf = [0u8; 32];
    let mut hasher = new_blake2b();
    hasher.update(root);
    hasher.update(&count.to_le_bytes());
    hasher.finalize(&mut buf);
    buf
}

/// Compute merkle root from vectors
pub fn calculate_merkle_root(leaves: Vec<[u8; 32]>) -> Result<[u8; 32], Error> {
    if leaves.is_empty() {
        return Ok(H256::zero().into());
    }
    let mut tree = SMT::<DefaultStore<H256>>::default();
    for (i, leaf) in leaves.into_iter().enumerate() {
        tree.update(H256::from_u32(i as u32), leaf.into())?;
    }
    Ok((*tree.root()).into())
}
