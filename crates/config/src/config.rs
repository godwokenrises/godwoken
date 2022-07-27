use ckb_fixed_hash::{H160, H256};
use gw_jsonrpc_types::{
    blockchain::{CellDep, Script},
    ckb_jsonrpc_types::JsonBytes,
    godwoken::{ChallengeTargetType, L2BlockCommittedInfo, RollupConfig},
};
use serde::{Deserialize, Serialize};
use std::{
    cmp::min,
    collections::{HashMap, HashSet},
    path::PathBuf,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Trace {
    Jaeger,
    TokioConsole,
}

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub node_mode: NodeMode,
    #[serde(default)]
    pub contract_log_config: ContractLogConfig,
    pub backend_switches: Vec<BackendSwitchConfig>,
    pub genesis: GenesisConfig,
    pub chain: ChainConfig,
    pub rpc_client: RPCClientConfig,
    pub rpc_server: RPCServerConfig,
    #[serde(default)]
    pub debug: DebugConfig,
    pub block_producer: Option<BlockProducerConfig>,
    #[serde(default)]
    pub offchain_validator: Option<OffChainValidatorConfig>,
    #[serde(default)]
    pub mem_pool: MemPoolConfig,
    #[serde(default)]
    pub db_block_validator: Option<DBBlockValidatorConfig>,
    pub store: StoreConfig,
    pub sentry_dsn: Option<String>,
    #[serde(default)]
    pub trace: Option<Trace>,
    #[serde(default)]
    pub consensus: ConsensusConfig,
    pub reload_config_github_url: Option<GithubConfigUrl>,
    #[serde(default)]
    pub dynamic_config: DynamicConfig,
    #[serde(default)]
    pub p2p_network_config: Option<P2PNetworkConfig>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "lowercase")]
pub enum RPCMethods {
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
    pub send_tx_rate_limit: Option<RPCRateLimit>,
}

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct RPCRateLimit {
    pub seconds: u64,
    pub lru_size: usize,
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
    pub eth_registry_validator_type_hash: H256,
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
pub struct ContractTypeScriptConfig {
    pub state_validator: Script,
    pub deposit_lock: Script,
    pub stake_lock: Script,
    pub custodian_lock: Script,
    pub withdrawal_lock: Script,
    pub challenge_lock: Script,
    pub l1_sudt: Script,
    pub omni_lock: Script,
    pub allowed_eoa_scripts: HashMap<H256, Script>,
    pub allowed_contract_scripts: HashMap<H256, Script>,
}

#[derive(Clone, Debug, Default)]
pub struct ContractsCellDep {
    pub rollup_cell_type: CellDep,
    pub deposit_cell_lock: CellDep,
    pub stake_cell_lock: CellDep,
    pub custodian_cell_lock: CellDep,
    pub withdrawal_cell_lock: CellDep,
    pub challenge_cell_lock: CellDep,
    pub l1_sudt_type: CellDep,
    pub omni_lock: CellDep,
    pub allowed_eoa_locks: HashMap<H256, CellDep>,
    pub allowed_contract_types: HashMap<H256, CellDep>,
}

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConsensusConfig {
    pub contract_type_scripts: ContractTypeScriptConfig,
}

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct RegistryAddressConfig {
    pub registry_id: u32,
    pub address: JsonBytes,
}
#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlockProducerConfig {
    #[serde(default = "default_check_mem_block_before_submit")]
    pub check_mem_block_before_submit: bool,
    pub block_producer: RegistryAddressConfig,
    pub rollup_config_cell_dep: CellDep,
    pub challenger_config: ChallengerConfig,
    #[serde(default = "default_block_producer_wallet")]
    pub wallet_config: Option<WalletConfig>,
    #[serde(default = "default_withdrawal_unlocker_wallet")]
    pub withdrawal_unlocker_wallet_config: Option<WalletConfig>,
}

const fn default_check_mem_block_before_submit() -> bool {
    false
}

const fn default_withdrawal_unlocker_wallet() -> Option<WalletConfig> {
    None
}

const fn default_block_producer_wallet() -> Option<WalletConfig> {
    None
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BackendType {
    Meta,
    Sudt,
    Polyjuice,
    EthAddrReg,
    Unknown,
}

impl Default for BackendType {
    fn default() -> Self {
        BackendType::Unknown
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BackendSwitchConfig {
    pub switch_height: u64,
    pub backends: Vec<BackendConfig>,
}

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct BackendConfig {
    pub validator_path: PathBuf,
    pub generator_path: PathBuf,
    pub validator_script_type_hash: H256,
    pub backend_type: BackendType,
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
        const EXPECTED_TX_UPPER_BOUND_CYCLES: u64 = 350000000u64;
        const DEFAULT_DEBUG_TX_DUMP_PATH: &str = "debug-tx-dump";

        Self {
            debug_tx_dump_path: DEFAULT_DEBUG_TX_DUMP_PATH.into(),
            output_l1_tx_cycles: true,
            expected_l1_tx_upper_bound_cycles: EXPECTED_TX_UPPER_BOUND_CYCLES,
            enable_debug_rpc: false,
        }
    }
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct P2PNetworkConfig {
    /// Multiaddr listen address, e.g. /ip4/1.2.3.4/tcp/443
    #[serde(default)]
    pub listen: Option<String>,
    /// Multiaddr dial addresses, e.g. /ip4/1.2.3.4/tcp/443
    #[serde(default)]
    pub dial: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PublishMemPoolConfig {
    pub hosts: Vec<String>,
    pub topic: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SubscribeMemPoolConfig {
    pub hosts: Vec<String>,
    pub topic: String,
    pub group: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemPoolConfig {
    pub execute_l2tx_max_cycles: u64,
    #[serde(default = "default_restore_path")]
    pub restore_path: PathBuf,
    pub publish: Option<PublishMemPoolConfig>,
    pub subscribe: Option<SubscribeMemPoolConfig>,
    #[serde(default)]
    pub mem_block: MemBlockConfig,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemBlockConfig {
    pub max_deposits: usize,
    pub max_withdrawals: usize,
    pub max_txs: usize,
    #[serde(
        default = "default_max_block_cycles_limit",
        with = "toml_u64_serde_workaround"
    )]
    pub max_cycles_limit: u64,
    #[serde(default = "default_syscall_cycles")]
    pub syscall_cycles: SyscallCyclesConfig,
}

const fn default_max_block_cycles_limit() -> u64 {
    u64::MAX
}

fn default_syscall_cycles() -> SyscallCyclesConfig {
    SyscallCyclesConfig::all_zero()
}

// Workaround: https://github.com/alexcrichton/toml-rs/issues/256
// Serialize to string instead
mod toml_u64_serde_workaround {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(val: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&val.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<u64, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: &str = Deserialize::deserialize(deserializer)?;
        s.parse::<u64>().map_err(serde::de::Error::custom)
    }
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
            restore_path: default_restore_path(),
            publish: None,
            subscribe: None,
            mem_block: MemBlockConfig::default(),
        }
    }
}

impl Default for MemBlockConfig {
    fn default() -> Self {
        Self {
            max_deposits: 100,
            max_withdrawals: 100,
            max_txs: 1000,
            max_cycles_limit: default_max_block_cycles_limit(),
            syscall_cycles: SyscallCyclesConfig::all_zero(),
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
    pub cache_size: Option<usize>,
    #[serde(default)]
    pub options_file: Option<PathBuf>,
    #[serde(default)]
    pub options: HashMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FeeConfig {
    // fee_rate: fee / cycles limit
    pub meta_cycles_limit: u64,
    // fee_rate: fee / cycles limit
    pub sudt_cycles_limit: u64,
    // fee_rate: fee / cycles_limit
    pub eth_addr_reg_cycles_limit: u64,
    // fee_rate: fee / cycles limit
    pub withdraw_cycles_limit: u64,
}

impl FeeConfig {
    pub fn minimal_tx_cycles_limit(&self) -> u64 {
        min(
            min(self.meta_cycles_limit, self.sudt_cycles_limit),
            self.eth_addr_reg_cycles_limit,
        )
    }
}

impl Default for FeeConfig {
    fn default() -> Self {
        // CKB default weight is 1000 / 1000
        Self {
            // 20K cycles unified for simple Godwoken native contracts
            meta_cycles_limit: 20000,
            sudt_cycles_limit: 20000,
            withdraw_cycles_limit: 20000,
            eth_addr_reg_cycles_limit: 20000, // 1176198 cycles used
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GithubConfigUrl {
    pub org: String,
    pub repo: String,
    pub branch: String,
    pub path: String,
    pub token: String,
}

// Configs in DynamicConfig can be hot reloaded from remote. But GithubConfigUrl must be setup.
#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct DynamicConfig {
    pub fee_config: FeeConfig,
    pub rpc_config: RPCConfig,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContractLogConfig {
    Default,       //print contract log in place
    Redirect,      //print all of the contract logs, send error logs to sentry
    RedirectError, //only print logs when the tx hit an error, send error logs to sentry
}

impl Default for ContractLogConfig {
    fn default() -> Self {
        ContractLogConfig::Default
    }
}

// Cycles config for all db related syscalls
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SyscallCyclesConfig {
    pub sys_store_cycles: u64,
    pub sys_load_cycles: u64,
    pub sys_create_cycles: u64,
    pub sys_load_account_script_cycles: u64,
    pub sys_store_data_cycles: u64,
    pub sys_load_data_cycles: u64,
    pub sys_get_block_hash_cycles: u64,
    pub sys_recover_account_cycles: u64,
    pub sys_log_cycles: u64,
}

impl SyscallCyclesConfig {
    pub fn all_zero() -> Self {
        SyscallCyclesConfig {
            sys_store_cycles: 0,
            sys_load_cycles: 0,
            sys_create_cycles: 0,
            sys_load_account_script_cycles: 0,
            sys_store_data_cycles: 0,
            sys_load_data_cycles: 0,
            sys_get_block_hash_cycles: 0,
            sys_recover_account_cycles: 0,
            sys_log_cycles: 0,
        }
    }
}
