use gw_types::packed::Script;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub chain: ChainConfig,
    pub store: StoreConfig,
    pub genesis: GenesisConfig,
    pub aggregator: Option<AggregatorConfig>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AggregatorConfig {
    pub account_id: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GenesisConfig {
    pub timestamp: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChainConfig {
    pub rollup_type_script: Script,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoreConfig {
    pub path: PathBuf,
}
