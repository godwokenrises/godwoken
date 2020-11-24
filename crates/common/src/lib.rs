#![cfg_attr(not(feature = "std"), no_std)]

pub mod builtins;
pub mod merkle_utils;
pub mod smt;
pub mod state;

// type aliase

pub type H256 = [u8; 32];

// re-exports

pub use gw_hash::blake2b;
pub use sparse_merkle_tree;

/// Common constants

pub const DEPOSITION_CODE_HASH: H256 = [0u8; 32];
pub const SUDT_CODE_HASH: H256 = [0u8; 32];
pub const ROLLUP_LOCK_CODE_HASH: H256 = [0u8; 32];
pub const CKB_TOKEN_ID: H256 = [0u8; 32];

cfg_if::cfg_if! {
    if #[cfg(feature = "std")] {
        use std::vec;
    } else {
        extern crate alloc;
        use alloc::vec;
    }
}
