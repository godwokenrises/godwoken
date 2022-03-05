use ckb_fixed_hash::H256;
use ckb_jsonrpc_types::{CellDep, Script};
use gw_config::GenesisConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug, Default)]
pub struct SetupConfig {
    pub l1_sudt_script_type_hash: H256,
    pub l1_sudt_cell_dep: CellDep,
    pub node_initial_ckb: u64,
    pub cells_lock: Script,
    pub burn_lock: Script,
    pub reward_lock: Script,
    pub omni_lock_config: OmniLockConfig,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug, Default)]
pub struct RollupDeploymentResult {
    pub tx_hash: H256,
    pub timestamp: u64,
    pub rollup_type_hash: H256,
    pub rollup_type_script: ckb_jsonrpc_types::Script,
    pub rollup_config: gw_jsonrpc_types::godwoken::RollupConfig,
    pub rollup_config_cell_dep: ckb_jsonrpc_types::CellDep,
    pub layer2_genesis_hash: H256,
    pub genesis_config: GenesisConfig,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
pub struct UserRollupConfig {
    pub l1_sudt_script_type_hash: H256,
    pub l1_sudt_cell_dep: CellDep,
    pub cells_lock: Script,
    pub burn_lock: Script,
    pub reward_lock: Script,
    pub required_staking_capacity: u64,
    pub challenge_maturity_blocks: u64,
    pub finality_blocks: u64,
    pub reward_burn_rate: u8,     // * reward_burn_rate / 100
    pub compatible_chain_id: u32, // compatible chain id
    pub allowed_eoa_type_hashes: Vec<H256>,
    pub allowed_contract_type_hashes: Vec<H256>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
pub struct DeployItem {
    pub script_type_hash: H256,
    pub cell_dep: CellDep,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
pub struct ScriptsDeploymentResult {
    pub custodian_lock: DeployItem,
    pub deposit_lock: DeployItem,
    pub withdrawal_lock: DeployItem,
    pub challenge_lock: DeployItem,
    pub stake_lock: DeployItem,
    pub state_validator: DeployItem,
    pub meta_contract_validator: DeployItem,
    pub l2_sudt_validator: DeployItem,
    pub eth_account_lock: DeployItem,
    pub tron_account_lock: DeployItem,
    pub polyjuice_validator: DeployItem,
    pub eth_addr_reg_validator: DeployItem,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
pub struct OmniLockConfig {
    pub cell_dep: CellDep,
    pub script_type_hash: H256,
    pub pubkey_h160: Option<[u8; 20]>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct BuildScriptsResult {
    pub programs: Programs,
    pub lock: Script,
    pub built_scripts: HashMap<String, PathBuf>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
pub struct Programs {
    // path: godwoken-scripts/build/release/custodian-lock
    pub custodian_lock: PathBuf,
    // path: godwoken-scripts/build/release/deposit-lock
    pub deposit_lock: PathBuf,
    // path: godwoken-scripts/build/release/withdrawal-lock
    pub withdrawal_lock: PathBuf,
    // path: godwoken-scripts/build/release/challenge-lock
    pub challenge_lock: PathBuf,
    // path: godwoken-scripts/build/release/stake-lock
    pub stake_lock: PathBuf,
    // path: godwoken-scripts/build/release/state-validator
    pub state_validator: PathBuf,
    // path: godwoken-scripts/c/build/sudt-validator
    pub l2_sudt_validator: PathBuf,

    // path: godwoken-scripts/c/build/account_locks/eth-account-lock
    pub eth_account_lock: PathBuf,
    // path: godwoken-scripts/c/build/account_locks/tron-account-lock
    pub tron_account_lock: PathBuf,

    // path: godwoken-scripts/c/build/meta-contract-validator
    pub meta_contract_validator: PathBuf,
    // path: godwoken-polyjuice/build/validator
    pub polyjuice_validator: PathBuf,
    // path: godwoken-polyjuice/build/eth_addr_reg_validator
    pub eth_addr_reg_validator: PathBuf,
}
