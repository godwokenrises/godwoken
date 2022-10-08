use once_cell::sync::Lazy;
use prometheus_client::{
    encoding::text::SendSyncEncodeMetric,
    metrics::{counter::Counter, gauge::Gauge},
    registry::Registry,
};

// Why don't Counter or Gauge have `const fn new() -> Self`?

pub static TRANSACTIONS: Lazy<Counter> = Lazy::new(Counter::default);
pub static DEPOSITS: Lazy<Counter> = Lazy::new(Counter::default);
pub static WITHDRAWALS: Lazy<Counter> = Lazy::new(Counter::default);
pub static BLOCK_HEIGHT: Lazy<Gauge> = Lazy::new(Gauge::default);

pub fn registry() -> Registry<Box<dyn SendSyncEncodeMetric>> {
    let mut registry: Registry<Box<dyn SendSyncEncodeMetric>> = Registry::with_prefix("gw");
    registry.register(
        "transactions",
        "number of packaged L2 transactions",
        Box::new(TRANSACTIONS.clone()),
    );
    registry.register(
        "deposits",
        "number of packaged deposits",
        Box::new(DEPOSITS.clone()),
    );
    registry.register(
        "withdrawals",
        "number of packaged withdrawals",
        Box::new(WITHDRAWALS.clone()),
    );
    registry.register(
        "block_height",
        "layer 2 block height",
        Box::new(BLOCK_HEIGHT.clone()),
    );
    registry
}
