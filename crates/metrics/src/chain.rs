use gw_telemetry::metric::{
    registry::Registry,
    OnceCell,
    {counter::Counter, gauge::Gauge},
};

pub fn chain() -> &'static ChainMetrics {
    static METRICS: OnceCell<ChainMetrics> = OnceCell::new();
    METRICS.get_or_init(|| {
        let metrics = ChainMetrics::default();
        let mut registry = crate::global_registry();
        metrics.register(registry.sub_registry_with_prefix("chain"));
        metrics
    })
}

#[derive(Default)]
pub struct ChainMetrics {
    pub transactions: Counter,
    pub deposits: Counter,
    pub withdrawals: Counter,
    pub block_height: Gauge,
}

impl ChainMetrics {
    fn register(&self, registry: &mut Registry) {
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
