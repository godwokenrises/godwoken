#![cfg_attr(not(feature = "std"), no_std)]

pub mod blake2b;
pub mod smt;
pub mod state;

// type aliase

pub type H256 = [u8; 32];

// re-exports

pub use sparse_merkle_tree;

/// Common constants

pub const DEPOSITION_CODE_HASH: H256 = [0u8; 32];
pub const SUDT_CODE_HASH: H256 = [0u8; 32];
pub const ROLLUP_LOCK_CODE_HASH: H256 = [0u8; 32];
pub const CKB_TOKEN_ID: H256 = [0u8; 32];
