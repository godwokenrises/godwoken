//! Generator handle layer2 transactions and blocks,
//! and generate new status that can be committed to layer1

pub mod account_lock_manage;
pub mod backend_manage;
pub mod builtin_scripts;
pub mod dummy_state;
pub mod error;
pub mod generator;
pub mod genesis;
pub mod sudt;
pub mod syscalls;
pub mod traits;
pub mod types;

#[cfg(test)]
mod tests;

// re-exports
pub use error::Error;
pub use generator::Generator;
pub use types::*;

pub(crate) fn code_hash(data: &[u8]) -> gw_common::H256 {
    let mut hasher = gw_common::blake2b::new_blake2b();
    hasher.update(data);
    let mut code_hash = [0u8; 32];
    hasher.finalize(&mut code_hash);
    code_hash.into()
}
