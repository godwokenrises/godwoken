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

use crate::{fork_config::BackendForkConfig, ForkConfig};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Trace {
    Jaeger,
    TokioConsole,
}

#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub node_mode: NodeMode,
    pub liveness_duration_secs: Option<u64>,
    #[serde(default)]
    pub contract_log_config: ContractLogConfig,
    pub debug_backend_forks: Option<Vec<BackendForkConfig>>,
    pub fork: ForkConfig,
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
    #[serde(default)]
    pub trace: Option<Trace>,
    #[serde(default)]
    pub consensus: ConsensusConfig,
    pub reload_config_github_url: Option<GithubConfigUrl>,
    #[serde(default)]
    pub dynamic_config: DynamicConfig,
    #[serde(default)]
    pub p2p_network_config: Option<P2PNetworkConfig>,
    #[serde(default)]
    pub sync_server: SyncServerConfig,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "lowercase")]
pub enum RPCMethods {
    PProf,
    Test,
    Debug,
}

#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RPCServerConfig {
    pub listen: String,
    #[serde(default)]
    pub enable_methods: HashSet<RPCMethods>,
}

#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RPCClientConfig {
    pub indexer_url: String,
    pub ckb_url: String,
}

#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RPCConfig {
    pub allowed_sudt_proxy_creator_account_id: Vec<u32>,
    pub sudt_proxy_code_hashes: Vec<H256>,
    pub allowed_polyjuice_contract_creator_address: Option<HashSet<H160>>,
    pub polyjuice_script_code_hash: Option<H256>,
    pub send_tx_rate_limit: Option<RPCRateLimit>,
}

#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RPCRateLimit {
    pub seconds: u64,
    pub lru_size: usize,
}

/// Onchain rollup cell config
#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChainConfig {
    /// Ignore invalid state caused by blocks
    #[serde(default)]
    pub skipped_invalid_block_list: Vec<H256>,
    pub genesis_committed_info: L2BlockCommittedInfo,
    pub rollup_type_script: Script,
}

/// Genesis config
#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenesisConfig {
    pub timestamp: u64,
    pub rollup_type_hash: H256,
    pub meta_contract_validator_type_hash: H256,
    pub eth_registry_validator_type_hash: H256,
    pub rollup_config: RollupConfig,
    // For load secp data and use in challenge transaction
    pub secp_data_dep: CellDep,
}

#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WalletConfig {
    pub privkey_path: PathBuf,
    pub lock: Script,
}

// NOTE: Rewards receiver lock must be different than lock in WalletConfig,
// since stake_capacity(minus burnt) + challenge_capacity - tx_fee will never
// bigger or equal than stake_capacity(minus burnt) + challenge_capacity.
// TODO: Support sudt stake ?
#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChallengerConfig {
    pub rewards_receiver_lock: Script,
    pub burn_lock: Script,
}

#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsensusConfig {
    pub contract_type_scripts: ContractTypeScriptConfig,
}

#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryAddressConfig {
    pub registry_id: u32,
    pub address: JsonBytes,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BlockProducerConfig {
    pub check_mem_block_before_submit: bool,
    pub fee_rate: u64,
    #[serde(flatten)]
    pub psc_config: PscConfig,
    pub block_producer: RegistryAddressConfig,
    pub rollup_config_cell_dep: CellDep,
    pub challenger_config: ChallengerConfig,
    pub wallet_config: Option<WalletConfig>,
    pub withdrawal_unlocker_wallet_config: Option<WalletConfig>,
}

impl Default for BlockProducerConfig {
    fn default() -> Self {
        BlockProducerConfig {
            check_mem_block_before_submit: false,
            fee_rate: 1000,
            psc_config: PscConfig::default(),
            block_producer: RegistryAddressConfig::default(),
            rollup_config_cell_dep: CellDep::default(),
            challenger_config: ChallengerConfig::default(),
            wallet_config: None,
            withdrawal_unlocker_wallet_config: None,
        }
    }
}

#[test]
fn test_default_block_producer_config() {
    let config: BlockProducerConfig = toml::from_str("").unwrap();
    assert_eq!(config, BlockProducerConfig::default());
    assert!(config.fee_rate > 0);
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PscConfig {
    /// Maximum number local blocks. Local blocks are blocks that have not been
    /// submitted to L1. Default is 5.
    pub local_limit: u64,
    /// Maximum number of submitted (but not confirmed) blocks. Default is 5.
    pub submitted_limit: u64,
    /// Minimum delay between blocks. Default is 8 seconds.
    pub block_interval_secs: u64,
}

impl Default for PscConfig {
    fn default() -> Self {
        Self {
            local_limit: 5,
            submitted_limit: 5,
            block_interval_secs: 8,
        }
    }
}

#[test]
fn test_psc_config_optional() {
    #[derive(Deserialize)]
    struct BiggerConfig {
        _x: i32,
        #[serde(flatten)]
        psc_config: PscConfig,
    }

    assert_eq!(
        toml::from_str::<BiggerConfig>("_x = 3").unwrap().psc_config,
        PscConfig::default()
    );
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct P2PNetworkConfig {
    /// Multiaddr listen address, e.g. /ip4/1.2.3.4/tcp/443
    pub listen: Option<String>,
    /// Multiaddr dial addresses, e.g. /ip4/1.2.3.4/tcp/443
    #[serde(default)]
    pub dial: Vec<String>,
    pub secret_key_path: Option<PathBuf>,
    pub allowed_peer_ids: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncServerConfig {
    pub buffer_capacity: u64,
    pub broadcast_channel_capacity: usize,
}

impl Default for SyncServerConfig {
    fn default() -> Self {
        Self {
            buffer_capacity: 16,
            broadcast_channel_capacity: 1024,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemPoolConfig {
    pub execute_l2tx_max_cycles: u64,
    #[serde(default = "default_restore_path")]
    pub restore_path: PathBuf,
    #[serde(default)]
    pub mem_block: MemBlockConfig,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemBlockConfig {
    pub max_deposits: usize,
    pub max_withdrawals: usize,
    pub max_txs: usize,
    #[serde(flatten)]
    pub deposit_timeout_config: DepositTimeoutConfig,
    #[serde(
        default = "default_max_block_cycles_limit",
        with = "toml_u64_serde_workaround"
    )]
    pub max_cycles_limit: u64,
    #[serde(default = "default_syscall_cycles")]
    pub syscall_cycles: SyscallCyclesConfig,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DepositTimeoutConfig {
    /// Only package deposits whose block timeout >= deposit_block_timeout.
    pub deposit_block_timeout: u64,
    /// Only package deposits whose timestamp timeout >= deposit_timestamp_timeout.
    pub deposit_timestamp_timeout: u64,
    /// Only package deposits whose epoch timeout >= deposit_epoch_timeout.
    pub deposit_epoch_timeout: u64,
    /// Only package deposits whose block number <= tip - deposit_minimum_blocks.
    pub deposit_minimal_blocks: u64,
}

impl Default for DepositTimeoutConfig {
    fn default() -> Self {
        Self {
            // 150 blocks, ~20 minutes.
            deposit_block_timeout: 150,
            // 20 minutes.
            deposit_timestamp_timeout: 1_200_000,
            // 1 epoch, about 4 hours, this option is supposed not actually used, so we simply set a value
            deposit_epoch_timeout: 1,
            deposit_minimal_blocks: 0,
        }
    }
}

const fn default_max_block_cycles_limit() -> u64 {
    u64::MAX
}

fn default_syscall_cycles() -> SyscallCyclesConfig {
    SyscallCyclesConfig::default()
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
            deposit_timeout_config: Default::default(),
            max_cycles_limit: default_max_block_cycles_limit(),
            syscall_cycles: SyscallCyclesConfig::default(),
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GithubConfigUrl {
    pub org: String,
    pub repo: String,
    pub branch: String,
    pub path: String,
    pub token: String,
}

// Configs in DynamicConfig can be hot reloaded from remote. But GithubConfigUrl must be setup.
#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicConfig {
    pub fee_config: FeeConfig,
    pub rpc_config: RPCConfig,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum ContractLogConfig {
    Verbose,
    Error,
}

impl Default for ContractLogConfig {
    fn default() -> Self {
        ContractLogConfig::Verbose
    }
}

// Cycles config for all db related syscalls
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    pub sys_bn_add_cycles: u64,
    pub sys_bn_mul_cycles: u64,
    pub sys_bn_fixed_pairing_cycles: u64,
    pub sys_bn_per_pairing_cycles: u64,
    pub sys_snapshot_cycles: u64,
    pub sys_revert_cycles: u64,
}

impl SyscallCyclesConfig {
    pub fn default() -> Self {
        SyscallCyclesConfig {
            sys_store_cycles: 50000,
            sys_load_cycles: 5000,
            sys_create_cycles: 50000,
            sys_load_account_script_cycles: 5000,
            sys_store_data_cycles: 50000,
            sys_load_data_cycles: 5000,
            sys_get_block_hash_cycles: 50000,
            sys_recover_account_cycles: 50000,
            sys_log_cycles: 50000,

            // default cycles of BN operations
            // estimated_cycles = 3 * (Gas Cost of EIP-1108)
            // see: https://eips.ethereum.org/EIPS/eip-1108
            sys_bn_add_cycles: 450,
            sys_bn_mul_cycles: 18_000,
            sys_bn_fixed_pairing_cycles: 135_000,
            sys_bn_per_pairing_cycles: 102_000,
            sys_snapshot_cycles: 2000,
            sys_revert_cycles: 2000,
        }
    }
}
