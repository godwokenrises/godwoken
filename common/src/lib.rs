#![cfg_attr(not(feature = "std"), no_std)]

pub mod blake2b;
pub mod smt;
pub mod state;

// re-exports

pub use sparse_merkle_tree;

/// Common constants

pub const DEPOSITION_CODE_HASH: [u8; 32] = [0u8; 32];
pub const SUDT_CODE_HASH: [u8; 32] = [0u8; 32];
pub const ROLLUP_LOCK_CODE_HASH: [u8; 32] = [0u8; 32];
pub const CKB_TOKEN_ID: [u8; 32] = [0u8; 32];
