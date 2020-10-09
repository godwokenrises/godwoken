//! Generator handle layer2 transactions and blocks,
//! and generate new status that can be committed to layer1

mod blake2b;
pub mod context;
mod error;
mod smt;
mod state;
mod syscalls;
#[cfg(test)]
mod tests;

// re-exports
pub use error::Error;
pub(crate) use godwoken_types::bytes;
pub use state::State;
