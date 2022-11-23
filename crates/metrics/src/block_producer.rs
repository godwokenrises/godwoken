use gw_telemetry::metric::{
    counter::Counter,
    gauge::Gauge,
    registry::{Registry, Unit},
    OnceCell,
};

pub fn block_producer() -> &'static BlockProducerMetrics {
    static METRICS: OnceCell<BlockProducerMetrics> = OnceCell::new();
    METRICS.get_or_init(|| {
        let metrics = BlockProducerMetrics::default();
        let mut registry = crate::global_registry();
        metrics.register(registry.sub_registry_with_prefix("block_producer"));
        metrics
    })
}

#[derive(Default)]
pub struct BlockProducerMetrics {
    pub resend: Counter,
    pub witness_size: Counter,
    pub tx_size: Counter,
    pub sync_buffer_len: Gauge,
    pub local_blocks: Gauge,
    pub submitted_blocks: Gauge,
}

impl BlockProducerMetrics {
    fn register(&self, registry: &mut Registry) {
        registry.register(
            "resend",
            "Number of times resending submission transactions",
            Box::new(self.resend.clone()),
        );
        registry.register_with_unit(
            "witness_size",
            "Block submission txs witness size",
            Unit::Bytes,
            Box::new(self.witness_size.clone()),
        );
        registry.register_with_unit(
            "tx_size",
            "Block submission txs size",
            Unit::Bytes,
            Box::new(self.tx_size.clone()),
        );
        registry.register(
            "sync_buffer_len",
            "Number of messages in the block sync receive buffer",
            Box::new(self.sync_buffer_len.clone()),
        );
        registry.register(
            "local_blocks",
            "Number of local blocks",
            Box::new(self.local_blocks.clone()),
        );
        registry.register(
            "submitted_blocks",
            "Number of submitted blocks",
            Box::new(self.submitted_blocks.clone()),
        );
    }
}
