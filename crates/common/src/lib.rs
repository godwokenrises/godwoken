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

/// constants
pub const CKB_SUDT_SCRIPT_ARGS: [u8; 32] = [0; 32];

cfg_if::cfg_if! {
    if #[cfg(feature = "std")] {
        use std::vec;
    } else {
        extern crate alloc;
        use alloc::vec;
    }
}

lazy_static::lazy_static! {
    pub static ref GLOBAL_VM_VERSION: smol::lock::Mutex<u32> = smol::lock::Mutex::new(0);
}
