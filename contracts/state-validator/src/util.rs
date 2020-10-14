use crate::error::Error as SMTError;
use alloc::vec::Vec;
use core::mem::size_of_val;
use gw_common::{
    blake2b::new_blake2b,
    smt::{default_store::DefaultStore, H256, SMT},
    state::ZERO,
};

/// Compute txs root from leaves
pub fn calculate_merkle_root(leaves: Vec<[u8; 32]>) -> Result<[u8; 32], SMTError> {
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

pub fn calculate_compacted_account_root(account_root: &[u8], count: u32) -> [u8; 32] {
    debug_assert_eq!(account_root.len(), 32);
    let mut buf = [0u8; 32];
    let mut hasher = new_blake2b();
    hasher.update(account_root);
    hasher.update(&count.to_le_bytes());
    hasher.finalize(&mut buf);
    buf
}
