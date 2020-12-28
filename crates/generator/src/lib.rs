//! Generator handle layer2 transactions and blocks,
//! and generate new status that can be committed to layer1

pub mod account_lock_manage;
pub mod backend_manage;
pub mod dummy_state;
pub mod error;
pub mod generator;
pub mod sudt;
pub mod syscalls;
#[cfg(test)]
mod tests;
pub mod traits;
mod types;

// re-exports
pub use error::Error;
pub use generator::Generator;
pub use types::*;
