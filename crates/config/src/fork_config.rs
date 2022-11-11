use std::path::PathBuf;

use ckb_fixed_hash::H256;
use serde::{Deserialize, Serialize};

use crate::constants::{
    L2TX_MAX_CYCLES_150M, L2TX_MAX_CYCLES_500M, MAX_READ_DATA_BYTES_LIMIT, MAX_TX_SIZE,
    MAX_WITHDRAWAL_SIZE, MAX_WRITE_DATA_BYTES_LIMIT,
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
pub struct BackendForkConfig {
    pub fork_height: u64,
    pub backends: Vec<BackendConfig>,
}

#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendConfig {
    pub validator_path: PathBuf,
    pub generator_path: PathBuf,
    pub validator_script_type_hash: H256,
    pub backend_type: BackendType,
}

/// Fork changes and activation heights.
#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForkConfig {
    /// Enable this to increase l2 tx cycles limit to 500M
    pub increase_max_l2_tx_cycles_to_500m: Option<u64>,
    pub backend_forks: Vec<BackendForkConfig>,
}

impl ForkConfig {
    /// Return l2 tx cycles limit by block height
    pub fn max_l2_tx_cycles(&self, block_number: u64) -> u64 {
        match self.increase_max_l2_tx_cycles_to_500m {
            None => L2TX_MAX_CYCLES_150M,
            Some(fork_number) if fork_number < block_number => L2TX_MAX_CYCLES_150M,
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
        MAX_WRITE_DATA_BYTES_LIMIT
    }

    pub fn max_read_data_bytes(&self, _block_number: u64) -> usize {
        MAX_READ_DATA_BYTES_LIMIT
    }
}
