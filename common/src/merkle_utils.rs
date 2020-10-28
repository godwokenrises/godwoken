use crate::vec::Vec;
use crate::{
    blake2b::new_blake2b,
    smt::{default_store::DefaultStore, Error, H256, SMT},
    state::ZERO,
};
use core::mem::size_of_val;

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
        return Ok(ZERO);
    }
    let mut tree = SMT::<DefaultStore<H256>>::default();
    for (i, leaf) in leaves.into_iter().enumerate() {
        let mut key = ZERO;
        let index = i as u32;
        key[0..size_of_val(&index)].copy_from_slice(&index.to_le_bytes());
        tree.update(key.into(), leaf.into())?;
    }
    Ok((*tree.root()).into())
}
