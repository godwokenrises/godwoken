//! Godwoken mem pool
//! MemPool keeps l2transactions & withdrawal requests in an order.
//! MemPool only do basic verification on l2transactions & withdrawal requests,
//! the block producer need to verify the fully verification itself.

pub mod batch;
mod constants;
pub mod custodian;
pub mod default_provider;
mod deposit;
mod mem_block;
pub mod pool;
pub mod save_restore;
pub mod traits;
mod types;
pub mod withdrawal;
