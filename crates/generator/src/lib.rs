//! Generator handle layer2 transactions and blocks,
//! and generate new status that can be committed to layer1

pub mod account_lock_manage;
pub mod backend_manage;
pub mod constants;
pub mod dummy_state;
pub mod erc20_creator_allowlist;
pub mod error;
pub mod generator;
pub mod genesis;
pub mod sudt;
pub mod syscalls;
pub mod traits;
pub mod types;
pub mod vm_cost_model;

#[cfg(feature = "generate-genesis-accounts")]
mod genesis_accounts;

#[cfg(test)]
mod tests;

// re-exports
pub use error::Error;
pub use generator::Generator;
pub use types::*;
