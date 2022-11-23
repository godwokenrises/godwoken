use gw_telemetry::metric::{
    counter::Counter,
    gauge::Gauge,
    registry::{Registry, Unit},
    Lazy,
};

static BLOCK_PRODUCER_METRICS: Lazy<BlockProducerMetrics> =
    Lazy::new(|| BlockProducerMetrics::default());

pub fn block_producer() -> &'static BlockProducerMetrics {
    &BLOCK_PRODUCER_METRICS
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
    pub(crate) fn register(&self, config: &crate::Config, registry: &mut Registry) {
        registry.register(
            "sync_buffer_len",
            "Number of messages in the block sync receive buffer",
            Box::new(self.sync_buffer_len.clone()),
        );

        if config.node_mode == gw_config::NodeMode::FullNode {
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
}
