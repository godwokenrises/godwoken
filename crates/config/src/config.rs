use ckb_fixed_hash::{H160, H256};
use gw_common::error::Error;
use gw_jsonrpc_types::{
    blockchain::{CellDep, Script},
    godwoken::{ChallengeTargetType, L2BlockCommittedInfo, RollupConfig, Uint32},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub node_mode: NodeMode,
    pub backends: Vec<BackendConfig>,
    pub genesis: GenesisConfig,
    pub chain: ChainConfig,
    pub rpc_client: RPCClientConfig,
    pub rpc_server: RPCServerConfig,
    #[serde(default)]
    pub rpc: RPCConfig,
    #[serde(default)]
    pub debug: DebugConfig,
    pub block_producer: Option<BlockProducerConfig>,
    pub web3_indexer: Option<Web3IndexerConfig>,
    #[serde(default)]
    pub offchain_validator: Option<OffChainValidatorConfig>,
    #[serde(default)]
    pub mem_pool: MemPoolConfig,
    #[serde(default)]
    pub db_block_validator: Option<DBBlockValidatorConfig>,
    #[serde(default)]
    pub store: StoreConfig,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RPCMethods {
    #[serde(rename = "pprof")]
    PProf,
    Test,
}

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct RPCServerConfig {
    pub listen: String,
    #[serde(default)]
    pub enable_methods: HashSet<RPCMethods>,
}

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct RPCClientConfig {
    pub indexer_url: String,
    pub ckb_url: String,
}

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct RPCConfig {
    pub allowed_sudt_proxy_creator_account_id: Vec<u32>,
    pub sudt_proxy_code_hashes: Vec<H256>,
    pub allowed_polyjuice_contract_creator_address: Option<HashSet<H160>>,
    pub polyjuice_script_code_hash: Option<H256>,
}

/// Onchain rollup cell config
#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChainConfig {
    /// Ignore invalid state caused by blocks
    #[serde(default)]
    pub skipped_invalid_block_list: Vec<H256>,
    pub genesis_committed_info: L2BlockCommittedInfo,
    pub rollup_type_script: Script,
}

/// Genesis config
#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

// NOTE: Rewards receiver lock must be different than lock in WalletConfig,
// since stake_capacity(minus burnt) + challenge_capacity - tx_fee will never
// bigger or equal than stake_capacity(minus burnt) + challenge_capacity.
// TODO: Support sudt stake ?
#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChallengerConfig {
    pub rewards_receiver_lock: Script,
    pub burn_lock: Script,
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
    pub challenge_cell_lock_dep: CellDep,
    pub l1_sudt_type_dep: CellDep,
    pub allowed_eoa_deps: HashMap<H256, CellDep>,
    pub allowed_contract_deps: HashMap<H256, CellDep>,
    pub challenger_config: ChallengerConfig,
    pub wallet_config: WalletConfig,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendType {
    Meta,
    Sudt,
    Polyjuice,
    Unknown,
}

impl Default for BackendType {
    fn default() -> Self {
        BackendType::Unknown
    }
}

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct BackendConfig {
    pub backend_type: BackendType,
    pub validator_path: PathBuf,
    pub generator_path: PathBuf,
    pub validator_script_type_hash: H256,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DebugConfig {
    pub output_l1_tx_cycles: bool,
    pub expected_l1_tx_upper_bound_cycles: u64,
    /// Directory to save debugging info of l1 transactions
    pub debug_tx_dump_path: PathBuf,
    #[serde(default = "default_enable_debug_rpc")]
    pub enable_debug_rpc: bool,
}

// Field default value for backward config file compitability
fn default_enable_debug_rpc() -> bool {
    false
}

impl Default for DebugConfig {
    fn default() -> Self {
        const EXPECTED_TX_UPPER_BOUND_CYCLES: u64 = 45000000u64;
        const DEFAULT_DEBUG_TX_DUMP_PATH: &str = "debug-tx-dump";

        Self {
            debug_tx_dump_path: DEFAULT_DEBUG_TX_DUMP_PATH.into(),
            output_l1_tx_cycles: true,
            expected_l1_tx_upper_bound_cycles: EXPECTED_TX_UPPER_BOUND_CYCLES,
            enable_debug_rpc: false,
        }
    }
}

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct Web3IndexerConfig {
    pub database_url: String,
    pub polyjuice_script_type_hash: H256,
    pub eth_account_lock_hash: H256,
    pub tron_account_lock_hash: Option<H256>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OffChainValidatorConfig {
    pub verify_withdrawal_signature: bool,
    pub verify_tx_signature: bool,
    pub verify_tx_execution: bool,
    pub verify_max_cycles: u64,
    pub dump_tx_on_failure: bool,
}

impl Default for OffChainValidatorConfig {
    fn default() -> Self {
        Self {
            verify_withdrawal_signature: true,
            verify_tx_signature: true,
            verify_tx_execution: true,
            verify_max_cycles: 70_000_000,
            dump_tx_on_failure: true,
        }
    }
}

/// Config the base/minimal fee rules
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct FeeConfig {
    meta_contract_fee_weight: u8,
    sudt_transfer_fee_weight: u8,
    withdraw_fee_weight: u8,
    /// HashMap<fee_sudt_id, fee_rate>
    ///
    /// fee_rate is known as gasPrice in Ethereum.
    ///
    /// The defaut fee_sudt_id is 1, which is CKB, and the ckb_fee_rate is denoted in shannons,
    /// which itself is a fractional denomination of CKBytes.
    ///   1 CKByte = 100,000,000 Shannons
    ///
    /// Note: The key of all TOML maps must be strings.
    fee_rates: HashMap<Uint32, u64>,
}
impl FeeConfig {
    pub fn is_supported_sudt(&self, sudt_id: u32) -> bool {
        if self.fee_rates.is_empty() {
            return true;
        }
        self.fee_rates.contains_key(&sudt_id.into())
    }
    pub fn get_fee_rate(&self, sudt_id: u32) -> Result<u128, Error> {
        if self.fee_rates.is_empty() {
            return Ok(0);
        }
        let fee_rate = self
            .fee_rates
            .get(&sudt_id.into())
            .ok_or(Error::UnsupportedFeeSudt)?;
        Ok(fee_rate.to_owned().into())
    }
    /// Get the minimal fee of meta contract
    pub fn meta_contract_minimum_fee(&self, sudt_id: u32) -> Result<u128, Error> {
        let fee_rate = self.get_fee_rate(sudt_id)?;
        Ok(fee_rate * u128::from(self.meta_contract_fee_weight))
    }
    /// Get the minimal fee of a native sudt transfer transaction
    pub fn sudt_transfer_minimum_fee(&self, sudt_id: u32) -> Result<u128, Error> {
        let fee_rate = self.get_fee_rate(sudt_id)?;
        Ok(fee_rate * u128::from(self.sudt_transfer_fee_weight))
    }
    /// Get the minimal fee of a withdrawal request
    pub fn withdrawal_minimum_fee(&self, sudt_id: u32) -> Result<u128, Error> {
        let fee_rate = self.get_fee_rate(sudt_id)?;
        Ok(fee_rate * u128::from(self.withdraw_fee_weight))
    }
    /// Get the minimal gasPrice of Polyjuice contract
    pub fn polyjuice_minimum_gas_price(&self, sudt_id: u32) -> Result<u128, Error> {
        self.get_fee_rate(sudt_id)
    }
}

impl From<FeeConfig> for gw_jsonrpc_types::godwoken::FeeConfig {
    fn from(fee_config: FeeConfig) -> gw_jsonrpc_types::godwoken::FeeConfig {
        let FeeConfig {
            fee_rates,
            meta_contract_fee_weight,
            sudt_transfer_fee_weight,
            withdraw_fee_weight,
        } = fee_config;
        let fee_rates = fee_rates
            .into_iter()
            .map(|(id, fee_rate)| (id, fee_rate.into()))
            .collect();
        gw_jsonrpc_types::godwoken::FeeConfig {
            fee_rates,
            meta_contract_fee_weight: u32::from(meta_contract_fee_weight).into(),
            sudt_transfer_fee_weight: u32::from(sudt_transfer_fee_weight).into(),
            withdraw_fee_weight: u32::from(withdraw_fee_weight).into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemPoolConfig {
    pub execute_l2tx_max_cycles: u64,
    pub submit_l2tx_max_cycles: u64,
    pub max_batch_channel_buffer_size: usize,
    pub max_batch_tx_withdrawal_size: usize,
    #[serde(default = "default_restore_path")]
    pub restore_path: PathBuf,
    #[serde(default)]
    pub fee_config: FeeConfig,
}

// Field default value for backward config file compitability
fn default_restore_path() -> PathBuf {
    const DEFAULT_RESTORE_PATH: &str = "mem_block";

    DEFAULT_RESTORE_PATH.into()
}

impl Default for MemPoolConfig {
    fn default() -> Self {
        Self {
            execute_l2tx_max_cycles: 100_000_000,
            submit_l2tx_max_cycles: 70_000_000,
            max_batch_channel_buffer_size: 2000,
            max_batch_tx_withdrawal_size: 200,
            restore_path: default_restore_path(),
            fee_config: Default::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeMode {
    FullNode,
    Test,
    ReadOnly,
}

impl Default for NodeMode {
    fn default() -> Self {
        NodeMode::ReadOnly
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DBBlockValidatorConfig {
    pub verify_max_cycles: u64,
    pub parallel_verify_blocks: bool,
    pub replace_scripts: Option<HashMap<H256, PathBuf>>,
    pub skip_targets: Option<HashSet<(u64, ChallengeTargetType, u32)>>,
}

impl Default for DBBlockValidatorConfig {
    fn default() -> Self {
        Self {
            verify_max_cycles: 7000_0000,
            replace_scripts: None,
            skip_targets: None,
            parallel_verify_blocks: true,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StoreConfig {
    #[serde(default)]
    pub path: PathBuf,
    #[serde(default)]
    pub options: HashMap<String, String>,
    #[serde(default)]
    pub options_file: Option<PathBuf>,
    #[serde(default)]
    pub cache_size: Option<usize>,
}
