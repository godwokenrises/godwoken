//! Chain is an off-chain actor
//! Chain composite multiple components to process the layer2 off-chain status:
//!
//! * Sync layer2 blocks from layer1, then send to generator
//! * Accept layer2 tx via RPC, then send to generator
//! * Watch the layer1 chain, send challenge if a invalid block is committed
//! * Submit new blocks to layer1(as an aggregator)

pub mod chain;
pub mod next_block_context;
#[cfg(test)]
mod tests;
pub mod mem_pool;
