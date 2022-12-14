use crate::{
    blake2b::new_blake2b,
    smt::{default_store::DefaultStore, Error, SMT, SMTH256},
    smt_h256_ext::SMTH256Ext,
};
use gw_types::core::H256;

// Calculate compacted account root
pub fn calculate_state_checkpoint(root: &H256, count: u32) -> H256 {
    let mut hash = [0u8; 32];
    let mut hasher = new_blake2b();
    hasher.update(root.as_slice());
    hasher.update(&count.to_le_bytes());
    hasher.finalize(&mut hash);
    hash.into()
}

/// Compute merkle root from vectors
pub fn calculate_merkle_root(leaves: Vec<H256>) -> Result<H256, Error> {
    if leaves.is_empty() {
        return Ok(H256::zero());
    }
    let mut tree = SMT::<DefaultStore<SMTH256>>::default();
    for (i, leaf) in leaves.into_iter().enumerate() {
        tree.update(SMTH256::from_u32(i as u32), SMTH256::from_h256(leaf))?;
    }
    Ok((*tree.root()).to_h256())
}
