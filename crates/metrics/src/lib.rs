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

use std::sync::RwLock;

use once_cell::sync::Lazy;
use prometheus_client::{encoding::text::SendSyncEncodeMetric, registry::Registry};

/// Global metrics registry.
pub static REGISTRY: Lazy<RwLock<Registry<Box<dyn SendSyncEncodeMetric>>>> =
    Lazy::new(|| Registry::with_prefix("gw").into());
