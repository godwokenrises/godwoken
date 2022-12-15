#![cfg_attr(not(feature = "std"), no_std)]

pub mod builtins;
pub mod ckb_decimal;
pub mod error;
pub mod merkle_utils;
pub mod registry;
pub mod state;
#[cfg(test)]
pub mod test_traits;
pub use gw_types::registry_address;

// re-exports
pub use gw_hash::blake2b;

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
