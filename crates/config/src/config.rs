use ckb_fixed_hash::H256;
use gw_jsonrpc_types::{
    blockchain::Script,
    ckb_jsonrpc_types::CellDep,
    godwoken::{HeaderInfo, RollupConfig},
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub chain: ChainConfig,
    pub store: StoreConfig,
    pub genesis: GenesisConfig,
    pub backends: Vec<BackendConfig>,
    pub block_producer: Option<BlockProducerConfig>,
    pub rollup_deployment: RollupDeploymentConfig,
    pub rpc_client: RPCClientConfig,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RPCClientConfig {
    pub indexer_url: String,
    pub ckb_url: String,
}

/// Onchain rollup cell config
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RollupDeploymentConfig {
    pub genesis_header: HeaderInfo,
}

/// Genesis config
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GenesisConfig {
    pub timestamp: u64,
    pub rollup_script_hash: H256,
    pub rollup_config: RollupConfig,
    pub meta_contract_validator_type_hash: H256,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WalletConfig {
    pub privkey_path: PathBuf,
    pub lock: Script,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlockProducerConfig {
    pub account_id: u32,
    pub wallet_config: WalletConfig,
    // cell deps
    pub rollup_cell_lock_dep: CellDep,
    pub rollup_cell_type_dep: CellDep,
    pub deposit_cell_lock_dep: CellDep,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChainConfig {
    pub rollup_type_script: Script,
    pub rollup_config: RollupConfig,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoreConfig {
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BackendConfig {
    pub validator_path: PathBuf,
    pub generator_path: PathBuf,
    pub validator_script_type_hash: H256,
}
