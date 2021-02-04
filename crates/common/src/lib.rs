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

pub const FINALIZE_BLOCKS: u64 = 10;

pub const DEPOSITION_LOCK_CODE_HASH: [u8; 32] = [0u8; 32];
pub const CUSTODIAN_LOCK_CODE_HASH: [u8; 32] = [0u8; 32];
pub const L2_SUDT_CODE_HASH: [u8; 32] = [0u8; 32];
pub const CKB_SUDT_SCRIPT_HASH: [u8; 32] = [
    127, 220, 53, 111, 197, 11, 67, 192, 98, 225, 52, 115, 150, 81, 152, 155, 24, 212, 187, 145,
    15, 66, 245, 53, 39, 191, 46, 195, 234, 199, 20, 224,
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
