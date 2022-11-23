use gw_telemetry::metric::{
    registry::Registry,
    Lazy,
    {counter::Counter, gauge::Gauge},
};

static CHAIN_METRICS: Lazy<ChainMetrics> = Lazy::new(|| ChainMetrics::default());

pub fn chain() -> &'static ChainMetrics {
    &CHAIN_METRICS
}

#[derive(Default)]
pub struct ChainMetrics {
    pub transactions: Counter,
    pub deposits: Counter,
    pub withdrawals: Counter,
    pub block_height: Gauge,
}

impl ChainMetrics {
    pub(crate) fn register(&self, config: &crate::Config, registry: &mut Registry) {
        registry.register(
            "block_height",
            "Number of the highest known block",
            Box::new(self.block_height.clone()),
        );

        if config.node_mode == gw_config::NodeMode::FullNode {
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
        }
    }
}
