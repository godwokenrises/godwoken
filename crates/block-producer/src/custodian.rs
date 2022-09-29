pub mod aggregator;
pub mod query;

pub const MAX_CUSTODIANS: usize = 50;
// Fit ckb-indexer output_capacity_range [inclusive, exclusive]
pub const MAX_CAPACITY: u64 = u64::MAX - 1;

pub use aggregator::{aggregate_balance, AvailableCustodians};
pub use query::{query_finalized_custodians, query_mergeable_custodians};
