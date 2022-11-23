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
use std::{collections::HashMap, sync::Arc};

use arc_swap::{ArcSwap, Guard};
use serde::Deserialize;
use smol_str::SmolStr;
use tracing::instrument;

use gw_common::H256;
use gw_telemetry::metric::{encoding, registry::Registry, Lazy};

// TODO: add to config.toml
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
type TextEncodeRegistry = Registry<Box<dyn encoding::text::SendSyncEncodeMetric>>;

static METRIC_REGISTRY: Lazy<ArcSwap<Option<TextEncodeRegistry>>> =
    Lazy::new(|| ArcSwap::from_pointee(None));
static CONFIG: Lazy<ArcSwap<Config>> = Lazy::new(|| ArcSwap::from_pointee(Config::default()));

pub fn init(config: &gw_config::Config) {
    let mut config = Config {
        node_mode: config.node_mode,
        ..Default::default()
    };
    debug_assert!(!config.custodian_enabled);

    let maybe_custodian_enable = std::env::var(ENV_METRIC_MONITOR_CUSTODIAN_ENABLE);
    if matches!(maybe_custodian_enable.as_deref(), Ok("true")) {
        config.custodian_enabled = true;
        config.custodian_map = Config::parse_map_from_env().unwrap_or_default();
    }

    let mut registry = Registry::with_prefix("gw");
    block_producer().register(&config, registry.sub_registry_with_prefix("block_producer"));
    chain().register(&config, registry.sub_registry_with_prefix("chain"));
    custodian().register(&config, registry.sub_registry_with_prefix("custodian"));
    rpc().register(&config, registry.sub_registry_with_prefix("rpc"));

    METRIC_REGISTRY.store(Arc::new(Some(registry)));
    CONFIG.store(Arc::new(config));
}

pub fn scrape(buf: &mut Vec<u8>) -> Result<(), std::io::Error> {
    if let Some(registry) = METRIC_REGISTRY.load().as_ref() {
        buf.reserve(2048);
        encoding::text::encode(buf, registry)?;
    }

    Ok(())
}

fn config() -> Guard<Arc<Config>> {
    CONFIG.load()
}

#[derive(Default)]
struct Config {
    node_mode: gw_config::NodeMode,
    custodian_enabled: bool,
    custodian_map: HashMap<H256, Custodian>,
}

impl Config {
    #[instrument(skip_all, err(Debug))]
    fn parse_map_from_env() -> Result<HashMap<H256, Custodian>, Box<dyn std::error::Error>> {
        #[derive(Deserialize, Debug)]
        struct JsonCustodian {
            pub symbol: String,
            pub type_hash: String,
            pub decimal: u32,
        }

        let json = std::env::var(ENV_METRIC_MONITOR_CUSTODIAN_VEC_JSON)?;
        tracing::info!("env metric monitor custodian vec json {}", json);
        let vec = serde_json::from_str::<Vec<JsonCustodian>>(&json)?;
        tracing::info!("parsed vec {:?}", vec);

        let to_custodian = vec.into_iter().map(|jc| -> Result<_, hex::FromHexError> {
            let mut buf = [0u8; 32];
            hex::decode_to_slice(&jc.type_hash, &mut buf)?;

            let c = Custodian {
                symbol: SmolStr::new_inline(&jc.symbol),
                type_hash: H256::from(buf),
                decimal: jc.decimal,
            };
            tracing::info!("monitor add {}", c.symbol);

            Ok((H256::from(buf), c))
        });

        Ok(to_custodian.collect::<Result<_, _>>()?)
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct Custodian {
    pub symbol: SmolStr,
    pub type_hash: H256,
    pub decimal: u32,
}
