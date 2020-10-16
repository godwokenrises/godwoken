//! Generator handle layer2 transactions and blocks,
//! and generate new status that can be committed to layer1

pub mod dummy_state;
mod error;
pub mod generator;
pub mod state_ext;
pub mod syscalls;
#[cfg(test)]
mod tests;
mod wrapped_store;

// re-exports
pub use error::Error;
pub use generator::Generator;
pub(crate) use gw_types::bytes;
