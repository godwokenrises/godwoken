use gw_otel::metric::{
    counter::Counter, encoding::text::Encode, family::Family, gauge::Gauge, prometheus_client,
    registry::Registry, OnceCell,
};

use crate::registry::Request;

pub fn rpc() -> &'static RPCMetrics {
    static METRICS: OnceCell<RPCMetrics> = OnceCell::new();
    METRICS.get_or_init(|| {
        let metrics = RPCMetrics::default();
        let mut registry = gw_otel::metric::global_registry();
        metrics.register(registry.sub_registry_with_prefix("rpc"));
        metrics
    })
}

#[derive(Default)]
pub struct RPCMetrics {
    execute_transactions: Family<ExecutionLabel, Counter>,
    in_queue_requests: Family<RequestLabel, Gauge>,
}

#[derive(Clone, Hash, PartialEq, Eq, Encode)]
pub enum RequestKind {
    Tx,
    Withdrawal,
}

impl RPCMetrics {
    pub fn execute_transactions(&self, exit_code: i8) -> Counter {
        self.execute_transactions
            .get_or_create(&ExecutionLabel { exit_code })
            .clone()
    }

    pub(crate) fn in_queue_requests(&self, req: &Request) -> Gauge {
        let kind = match req {
            Request::Tx(_) => RequestKind::Tx,
            Request::Withdrawal(_) => RequestKind::Withdrawal,
        };

        self.in_queue_requests
            .get_or_create(&RequestLabel { kind })
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
            Box::new(self.in_queue_requests.clone()),
        );
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
