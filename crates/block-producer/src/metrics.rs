use prometheus_client::{
    metrics::counter::Counter,
    registry::{Registry, Unit},
};

#[derive(Default)]
pub struct PSCMetrics {
    pub resend: Counter,
    pub witness_size: Counter,
    pub tx_size: Counter,
}

impl PSCMetrics {
    pub fn register(&self, registry: &mut Registry) {
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
    }
}
