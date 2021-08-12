//! Godwoken metrics configs and utils. This crate uses opentelemetry to collect local metrics
//! and sends to `opentelemeter-collector` periodically.
mod config;
#[cfg(feature = "metrics")]
mod opentelemeter;
pub mod utils;

pub use crate::config::init_meter as config;
