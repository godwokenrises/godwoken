//! Generator handle layer2 transactions and blocks,
//! and generate new status that can be committed to layer1

pub mod account_lock_manage;
pub mod backend_manage;
pub mod constants;
pub mod error;
pub mod generator;
pub mod genesis;
pub mod sudt;
pub mod syscalls;
pub mod traits;
pub mod typed_transaction;
pub mod types;
pub mod utils;
pub mod verification;
pub mod vm_cost_model;

#[cfg(test)]
mod tests;

// re-exports
pub use arc_swap::*;
pub use error::Error;
pub use generator::Generator;
pub use types::*;
