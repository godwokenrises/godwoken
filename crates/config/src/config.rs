use ckb_fixed_hash::H256;
use gw_types::packed::{RollupConfig, Script};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub chain: ChainConfig,
    pub store: StoreConfig,
    pub genesis: GenesisConfig,
    pub block_producer: Option<BlockProducerConfig>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BlockProducerConfig {
    pub account_id: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GenesisConfig {
    pub timestamp: u64,
    pub meta_contract_validator_type_hash: H256,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChainConfig {
    pub rollup_type_script: Script,
    pub rollup_config: RollupConfig,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoreConfig {
    pub path: PathBuf,
}
