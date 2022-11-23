//! Global metrics registry.
//!
//! ## Convention for metrics in godwoken:
//!
//! Each crate/module/component can define their own metrics and register them
//! to the global `REGISTRY`. To avoid naming conflict, each of them SHOULD use
//! a unique prefix, e.g. the crate name or component name.
//!
//! If it makes sense to define a metrics struct, e.g. when there are a few
//! related metrics and they usually change together, it SHOULD live in a
//! separate metrics module. See the metrics module in gw-chain for an example.
//!
//! When you add/modify some metrics, make sure to update the metrics document
//! in docs/metrics.md.
//!
use std::sync::{RwLock, RwLockWriteGuard};

use gw_telemetry::metric::{encoding, registry::Registry, Lazy};

const ENV_METRIC_MONITOR_CUSTODIAN_ENABLE: &str = "METRIC_MONITOR_CUSTODIAN_ENABLE";
const ENV_METRIC_MONITOR_CUSTODIAN_VEC_JSON: &str = "METRIC_MONITOR_CUSTODIAN_VEC_JSON";

pub mod block_producer;
pub mod chain;
pub mod custodian;
pub mod rpc;

pub use block_producer::block_producer;
pub use chain::chain;
pub use custodian::custodian;
pub use rpc::rpc;

/// Global metrics registry.
pub static REGISTRY: Lazy<RwLock<Registry<Box<dyn encoding::text::SendSyncEncodeMetric>>>> =
    Lazy::new(|| Registry::with_prefix("gw").into());

pub fn global_registry<'a>(
) -> RwLockWriteGuard<'a, Registry<Box<dyn encoding::text::SendSyncEncodeMetric>>> {
    REGISTRY.write().unwrap()
}

pub fn scrape(buf: &mut Vec<u8>) -> Result<(), std::io::Error> {
    buf.reserve(2048);
    encoding::text::encode(buf, &*REGISTRY.read().unwrap())?;
    Ok(())
}
