use prometheus_client::{
    metrics::{counter::Counter, gauge::Gauge},
    registry::Registry,
};

#[derive(Default)]
pub struct ChainMetrics {
    pub transactions: Counter,
    pub deposits: Counter,
    pub withdrawals: Counter,
    pub block_height: Gauge,
}

impl ChainMetrics {
    pub fn register(&self, registry: &mut Registry) {
        registry.register(
            "transactions",
            "Number of packaged L2 transactions",
            Box::new(self.transactions.clone()),
        );
        registry.register(
            "deposits",
            "Number of packaged deposits",
            Box::new(self.deposits.clone()),
        );
        registry.register(
            "withdrawals",
            "Number of packaged withdrawals",
            Box::new(self.withdrawals.clone()),
        );
        registry.register(
            "block_height",
            "Number of the highest known block",
            Box::new(self.block_height.clone()),
        );
    }
}
