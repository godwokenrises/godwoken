/// The `ckb-sdk` crate contains too many dependencies.
/// So we move some source code from https://github.com/nervosnetwork/ckb-sdk-rust
pub mod constants;
mod types;

pub use types::{Address, AddressPayload, HumanCapacity, NetworkType};
