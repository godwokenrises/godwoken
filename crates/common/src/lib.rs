#![cfg_attr(not(feature = "std"), no_std)]

pub mod builtins;
pub mod h256_ext;
pub mod merkle_utils;
pub mod smt;
pub mod state;

// re-exports

pub use gw_hash::blake2b;
pub use h256_ext::H256;
pub use sparse_merkle_tree;

/// Common constants

pub const DEPOSITION_CODE_HASH: [u8; 32] = [0u8; 32];
pub const SUDT_CODE_HASH: [u8; 32] = [0u8; 32];
pub const ACCOUNT_LOCK_CODE_HASH: [u8; 32] = [0u8; 32];
pub const ROLLUP_LOCK_CODE_HASH: [u8; 32] = [0u8; 32];
pub const CKB_TOKEN_ID: [u8; 32] = [0u8; 32];

cfg_if::cfg_if! {
    if #[cfg(feature = "std")] {
        use std::vec;
    } else {
        extern crate alloc;
        use alloc::vec;
    }
}
