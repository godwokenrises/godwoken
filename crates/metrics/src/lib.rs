//! Metrics.
//!
//! Global metrics that are always available should be declared and registered
//! here.
//!
//! Additional metrics can be registered by taking a write lock of global
//! `REGISTRY`.

use std::sync::RwLock;

use once_cell::sync::Lazy;
use prometheus_client::{
    encoding::text::SendSyncEncodeMetric,
    metrics::{counter::Counter, gauge::Gauge},
    registry::Registry,
};

#[derive(Default)]
pub struct ChainMetrics {
    transactions: Counter,
    deposits: Counter,
    withdrawals: Counter,
    block_height: Gauge,
}

impl ChainMetrics {
    pub fn transactions(&self) -> &Counter {
        &self.transactions
    }
    pub fn deposits(&self) -> &Counter {
        &self.deposits
    }
    pub fn withdrawals(&self) -> &Counter {
        &self.withdrawals
    }
    pub fn block_height(&self) -> &Gauge {
        &self.block_height
    }
}

static CHAIN_METRICS: Lazy<ChainMetrics> = Lazy::new(Default::default);

/// Global metrics registry.
pub static REGISTRY: Lazy<RwLock<Registry<Box<dyn SendSyncEncodeMetric>>>> = Lazy::new(|| {
    let mut registry: Registry<Box<dyn SendSyncEncodeMetric>> = Registry::with_prefix("gw");
    let chain_metrics = &*CHAIN_METRICS;
    registry.register(
        "transactions",
        "number of packaged L2 transactions",
        Box::new(chain_metrics.transactions().clone()),
    );
    registry.register(
        "deposits",
        "number of packaged deposits",
        Box::new(chain_metrics.deposits().clone()),
    );
    registry.register(
        "withdrawals",
        "number of packaged withdrawals",
        Box::new(chain_metrics.withdrawals().clone()),
    );
    registry.register(
        "block_height",
        "layer 2 block height",
        Box::new(chain_metrics.block_height().clone()),
    );
    registry.into()
});

/// Global chain metrics.
pub fn chain_metrics() -> &'static ChainMetrics {
    &*CHAIN_METRICS
}
