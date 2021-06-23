use ckb_fixed_hash::H256;
use gw_jsonrpc_types::{
    blockchain::{CellDep, Script},
    godwoken::{L2BlockCommittedInfo, RollupConfig},
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub test_mode: TestMode,
    pub backends: Vec<BackendConfig>,
    pub store: StoreConfig,
    pub genesis: GenesisConfig,
    pub chain: ChainConfig,
    pub rpc_client: RPCClientConfig,
    pub rpc_server: RPCServerConfig,
    pub block_producer: Option<BlockProducerConfig>,
    pub web3_indexer: Option<Web3IndexerConfig>,
}

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct RPCServerConfig {
    pub listen: String,
}

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct RPCClientConfig {
    pub indexer_url: String,
    pub ckb_url: String,
}

/// Onchain rollup cell config
#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChainConfig {
    pub genesis_committed_info: L2BlockCommittedInfo,
    pub rollup_type_script: Script,
}

/// Genesis config
#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct GenesisConfig {
    pub timestamp: u64,
    pub rollup_type_hash: H256,
    pub meta_contract_validator_type_hash: H256,
    pub rollup_config: RollupConfig,
    // For load secp data and use in challenge transaction
    pub secp_data_dep: CellDep,
}

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct WalletConfig {
    pub privkey_path: PathBuf,
    pub lock: Script,
}

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlockProducerConfig {
    pub account_id: u32,
    // cell deps
    pub rollup_cell_type_dep: CellDep,
    pub rollup_config_cell_dep: CellDep,
    pub deposit_cell_lock_dep: CellDep,
    pub stake_cell_lock_dep: CellDep,
    pub poa_lock_dep: CellDep,
    pub poa_state_dep: CellDep,
    pub custodian_cell_lock_dep: CellDep,
    pub withdrawal_cell_lock_dep: CellDep,
    pub l1_sudt_type_dep: CellDep,
    pub wallet_config: WalletConfig,
}

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoreConfig {
    pub path: PathBuf,
}

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct BackendConfig {
    pub validator_path: PathBuf,
    pub generator_path: PathBuf,
    pub validator_script_type_hash: H256,
}

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct Web3IndexerConfig {
    pub database_url: String,
    pub polyjuice_script_type_hash: H256,
    pub eth_account_lock_hash: H256,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TestMode {
    Enable,
    Disable,
}

impl Default for TestMode {
    fn default() -> Self {
        TestMode::Disable
    }
}
