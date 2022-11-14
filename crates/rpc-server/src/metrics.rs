use gw_otel::metric::{
    counter::Counter, encoding::text::Encode, family::Family, gauge::Gauge, registry::Registry,
    Lazy,
};

pub static RPC_METRICS: Lazy<RPCMetrics> = Lazy::new(|| {
    let metrics = RPCMetrics::default();
    let mut registry = gw_otel::metric::global();
    metrics.register(&mut registry.sub_registry_with_prefix("rpc"));
    metrics
});

#[derive(Default)]
pub struct RPCMetrics {
    execute_transactions: Family<RunResultLabel, Counter>,
    pub queue_len: Gauge,
}

impl RPCMetrics {
    pub fn execute_transactions(&self, exit_code: i8) -> Counter {
        self.execute_transactions
            .get_or_create(&RunResultLabel { exit_code })
            .clone()
    }

    fn register(&self, registry: &mut Registry) {
        registry.register(
            "execute_transactions",
            "Number of execute_transaction requests",
            Box::new(self.execute_transactions.clone()),
        );
        registry.register(
            "in_queue_requests",
            "Number of in queue requests",
            Box::new(self.queue_len.clone()),
        );
    }
}

// Label for the execute_transactions metric.
#[derive(Hash, Clone, Eq, PartialEq)]
struct RunResultLabel {
    exit_code: i8,
}

// Manual impl because i8 does not implement Encode.
impl Encode for RunResultLabel {
    fn encode(&self, writer: &mut dyn std::io::Write) -> Result<(), std::io::Error> {
        write!(writer, "exit_code=\"{}\"", self.exit_code)
    }
}
