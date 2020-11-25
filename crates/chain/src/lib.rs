//! Chain is an off-chain actor
//! Chain composite multiple components to process the layer2 off-chain status:
//!
//! * Sync layer2 blocks from layer1, then send to generator
//! * Accept layer2 tx via RPC, then send to generator
//! * Watch the layer1 chain, send challenge if a invalid block is committed
//! * Submit new blocks to layer1(as an aggregator)

pub mod chain;
pub mod consensus;
mod crypto;
pub mod genesis;
pub mod state_impl;
pub mod tx_pool;
