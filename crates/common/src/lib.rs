#![cfg_attr(not(feature = "std"), no_std)]

pub mod builtins;
pub mod error;
pub mod h256_ext;
pub mod merkle_utils;
pub mod smt;
pub mod state;

// re-exports

pub use gw_hash::blake2b;
pub use h256_ext::H256;
pub use sparse_merkle_tree;

/// Common constants

pub const FINALIZE_BLOCKS: u64 = 1000;

pub const DEPOSITION_LOCK_CODE_HASH: [u8; 32] = [0u8; 32];
pub const CUSTODIAN_LOCK_CODE_HASH: [u8; 32] = [0u8; 32];
pub const L2_SUDT_CODE_HASH: [u8; 32] = [0u8; 32];
pub const CKB_SUDT_SCRIPT_HASH: [u8; 32] = [
    151, 233, 213, 45, 56, 134, 185, 114, 35, 251, 50, 238, 245, 166, 195, 91, 193, 202, 198, 116,
    4, 250, 156, 8, 135, 226, 57, 57, 238, 108, 41, 198,
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
