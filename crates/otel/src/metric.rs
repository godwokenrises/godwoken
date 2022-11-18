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

pub use once_cell::sync::{Lazy, OnceCell};
pub use prometheus_client;
pub use prometheus_client::{encoding, metrics::*, registry};

use encoding::text::SendSyncEncodeMetric;
use registry::Registry;

/// Global metrics registry.
pub static REGISTRY: Lazy<RwLock<Registry<Box<dyn SendSyncEncodeMetric>>>> =
    Lazy::new(|| Registry::with_prefix("gw").into());

pub fn global<'a>() -> RwLockWriteGuard<'a, Registry<Box<dyn SendSyncEncodeMetric>>> {
    REGISTRY.write().unwrap()
}

pub fn scrape(buf: &mut Vec<u8>) -> Result<(), std::io::Error> {
    buf.reserve(2048);
    encoding::text::encode(buf, &*REGISTRY.read().unwrap())?;
    Ok(())
}
