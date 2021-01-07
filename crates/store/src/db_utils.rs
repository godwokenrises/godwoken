use gw_types::{packed::Byte32, prelude::*};

/// build tx key
pub fn build_transaction_key(block_hash: Byte32, index: u32) -> [u8; 36] {
    let mut key = [0u8; 36];
    key[..32].copy_from_slice(block_hash.as_slice());
    // use BE, so we have a sorted bytes representation
    key[32..].copy_from_slice(&index.to_be_bytes());
    key
}
