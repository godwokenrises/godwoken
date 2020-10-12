//! Generator handle layer2 transactions and blocks,
//! and generate new status that can be committed to layer1

mod blake2b;
mod error;
mod generator;
pub mod smt;
mod state;
pub mod syscalls;
#[cfg(test)]
mod tests;

// re-exports
pub use error::Error;
pub use generator::Generator;
pub(crate) use gw_types::bytes;
pub use state::State;
