use std::path::PathBuf;

use ckb_fixed_hash::H256;
use serde::{Deserialize, Serialize};

use crate::constants::{
    L2TX_MAX_CYCLES_150M, L2TX_MAX_CYCLES_500M, MAX_TOTAL_READ_DATA_BYTES, MAX_TX_SIZE,
    MAX_WITHDRAWAL_SIZE, MAX_WRITE_DATA_BYTES,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendForkConfig {
    pub fork_height: u64,
    pub backends: Vec<BackendConfig>,
}

#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendConfig {
    pub validator_path: PathBuf,
    pub generator_path: PathBuf,
    pub validator_script_type_hash: H256,
    pub backend_type: BackendType,
}

/// Fork changes and activation heights.
#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForkConfig {
    /// Enable this to increase l2 tx cycles limit to 500M
    pub increase_max_l2_tx_cycles_to_500m: Option<u64>,

    /// Bump GlobalState.version from v1 to v2.
    /// Fork changes:
    ///   - Optimize finality mechanism
    ///   - Remove `state_checkpoints` from RawL2Block
    pub upgrade_global_state_version_to_v2: Option<u64>,

    /// Backend fork configs
    pub backend_forks: Vec<BackendForkConfig>,
}

impl ForkConfig {
    /// Returns the version of global state for `block_number`.
    pub fn global_state_version(&self, block_number: u64) -> u8 {
        match self.upgrade_global_state_version_to_v2 {
            None => 1,
            Some(fork_number) if block_number < fork_number => 1,
            Some(_) => 2,
        }
    }

    /// Returns if use timestamp as timepoint
    pub fn use_timestamp_as_timepoint(&self, block_number: u64) -> bool {
        self.global_state_version(block_number) >= 2
    }

    /// Returns if we should enforce the correctness of `RawL2Block.state_checkpoint_list`.
    pub fn enforce_correctness_of_state_checkpoint_list(&self, block_number: u64) -> bool {
        self.global_state_version(block_number) <= 1
    }

    /// Return l2 tx cycles limit by block height
    pub fn max_l2_tx_cycles(&self, block_number: u64) -> u64 {
        match self.increase_max_l2_tx_cycles_to_500m {
            None => L2TX_MAX_CYCLES_150M,
            Some(fork_number) if block_number < fork_number => L2TX_MAX_CYCLES_150M,
            Some(_) => L2TX_MAX_CYCLES_500M,
        }
    }

    pub fn max_tx_size(&self, _block_number: u64) -> usize {
        MAX_TX_SIZE
    }

    pub fn max_withdrawal_size(&self, _block_number: u64) -> usize {
        MAX_WITHDRAWAL_SIZE
    }

    pub fn max_write_data_bytes(&self, _block_number: u64) -> usize {
        MAX_WRITE_DATA_BYTES
    }

    pub fn max_total_read_data_bytes(&self, _block_number: u64) -> usize {
        MAX_TOTAL_READ_DATA_BYTES
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        constants::{L2TX_MAX_CYCLES_150M, L2TX_MAX_CYCLES_500M},
        ForkConfig,
    };

    #[test]
    fn test_disable_fork() {
        let fork = ForkConfig::default();
        assert_eq!(fork.max_l2_tx_cycles(0), L2TX_MAX_CYCLES_150M);
        assert_eq!(fork.max_l2_tx_cycles(100), L2TX_MAX_CYCLES_150M);
        assert_eq!(fork.max_l2_tx_cycles(u64::MAX), L2TX_MAX_CYCLES_150M);
    }

    #[test]
    fn test_active_fork() {
        let fork = ForkConfig {
            increase_max_l2_tx_cycles_to_500m: Some(42),
            ..Default::default()
        };
        assert_eq!(fork.max_l2_tx_cycles(0), L2TX_MAX_CYCLES_150M);
        assert_eq!(fork.max_l2_tx_cycles(41), L2TX_MAX_CYCLES_150M);
        assert_eq!(fork.max_l2_tx_cycles(42), L2TX_MAX_CYCLES_500M);
        assert_eq!(fork.max_l2_tx_cycles(100), L2TX_MAX_CYCLES_500M);
        assert_eq!(fork.max_l2_tx_cycles(u64::MAX), L2TX_MAX_CYCLES_500M);
    }
}
