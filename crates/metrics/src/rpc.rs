use gw_telemetry::metric::{
    counter::Counter, encoding::text::Encode, family::Family, gauge::Gauge, prometheus_client,
    registry::Registry, Lazy,
};

static RPC_METRICS: Lazy<RPCMetrics> = Lazy::new(|| RPCMetrics::default());

pub fn rpc() -> &'static RPCMetrics {
    &RPC_METRICS
}

#[derive(Clone, Hash, PartialEq, Eq, Encode)]
pub enum RequestKind {
    Tx,
    Withdrawal,
}

#[derive(Default)]
pub struct RPCMetrics {
    execute_transactions: Family<ExecutionLabel, Counter>,
    in_queue_requests: Family<RequestLabel, Gauge>,
}

impl RPCMetrics {
    pub(crate) fn register(&self, config: &crate::Config, registry: &mut Registry) {
        registry.register(
            "execute_transactions",
            "Number of execute_transaction requests",
            Box::new(self.execute_transactions.clone()),
        );

        if config.node_mode == gw_config::NodeMode::FullNode {
            registry.register(
                "in_queue_requests",
                "Number of in queue requests",
                Box::new(self.in_queue_requests.clone()),
            );
        }
    }

    pub fn execute_transactions(&self, exit_code: i8) -> Counter {
        self.execute_transactions
            .get_or_create(&ExecutionLabel { exit_code })
            .clone()
    }

    pub fn in_queue_requests(&self, kind: RequestKind) -> Gauge {
        self.in_queue_requests
            .get_or_create(&RequestLabel { kind })
            .clone()
    }
}

// Label for the execute_transactions metric.
#[derive(Hash, Clone, Eq, PartialEq)]
struct ExecutionLabel {
    exit_code: i8,
}

// Manual impl because i8 does not implement Encode.
impl Encode for ExecutionLabel {
    fn encode(&self, writer: &mut dyn std::io::Write) -> Result<(), std::io::Error> {
        write!(writer, "exit_code=\"{}\"", self.exit_code)
    }
}

#[derive(Clone, Hash, PartialEq, Eq, Encode)]
struct RequestLabel {
    kind: RequestKind,
}
