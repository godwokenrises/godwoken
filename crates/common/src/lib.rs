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
    184, 77, 129, 128, 107, 82, 41, 144, 123, 102, 187, 144, 60, 28, 67, 38, 13, 58, 155, 0, 13,
    242, 255, 113, 167, 248, 112, 241, 195, 15, 146, 119,
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
