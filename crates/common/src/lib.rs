#![cfg_attr(not(feature = "std"), no_std)]

pub mod builtin_scripts;
pub mod builtins;
pub mod error;
pub mod h256_ext;
pub mod merkle_utils;
pub mod smt;
pub mod state;
pub mod sudt;
pub mod traits;

// re-exports

pub use gw_hash::blake2b;
pub use h256_ext::H256;
pub use sparse_merkle_tree;
pub use traits::CodeStore;

/// Common constants

pub const FINALIZE_BLOCKS: u64 = 1000;

pub const DEPOSITION_LOCK_CODE_HASH: [u8; 32] = [0u8; 32];
pub const CUSTODIAN_LOCK_CODE_HASH: [u8; 32] = [0u8; 32];
pub const L2_SUDT_CODE_HASH: [u8; 32] = [0u8; 32];
pub const CKB_SUDT_SCRIPT_HASH: [u8; 32] = [
    114, 233, 26, 217, 186, 181, 233, 82, 106, 121, 142, 136, 234, 239, 214, 5, 41, 1, 193, 15, 45,
    4, 64, 99, 9, 165, 51, 98, 17, 119, 222, 69,
];
pub const CKB_SUDT_SCRIPT_ARGS: [u8; 32] = [0; 32];
pub const ACCOUNT_LOCK_CODE_HASH: [u8; 32] = [0u8; 32];
pub const ROLLUP_LOCK_CODE_HASH: [u8; 32] = [0u8; 32];

cfg_if::cfg_if! {
    if #[cfg(feature = "std")] {
        use std::vec;
    } else {
        extern crate alloc;
        use alloc::vec;
    }
}

pub fn code_hash(data: &[u8]) -> crate::H256 {
    let mut hasher = crate::blake2b::new_blake2b();
    hasher.update(data);
    let mut code_hash = [0u8; 32];
    hasher.finalize(&mut code_hash);
    code_hash.into()
}
