use crate::packed::{AccountMerkleState, Byte32, LogItem, TransactionKey, TxReceipt, CellOutput, Script};
use crate::prelude::*;
use sparse_merkle_tree::H256;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Default)]
pub struct RunResult {
    pub read_values: HashMap<H256, H256>,
    pub write_values: HashMap<H256, H256>,
    pub return_data: Vec<u8>,
    pub account_count: Option<u32>,
    pub new_scripts: HashMap<H256, Vec<u8>>,
    pub get_scripts: HashSet<Vec<u8>>,
    pub write_data: HashMap<H256, Vec<u8>>,
    // data hash -> data full size
    pub read_data: HashMap<H256, Vec<u8>>,
    // log data
    pub logs: Vec<LogItem>,
    // used cycles
    pub used_cycles: u64,
}

impl CellOutput {
    pub fn occupied_capacity(&self, data_capacity: usize) -> ckb_types::core::CapacityResult<u64> {
        let output = ckb_types::packed::CellOutput::new_unchecked(self.as_bytes());
        output
            .occupied_capacity(ckb_types::core::Capacity::bytes(data_capacity)?)
            .map(|c| c.as_u64())
    }
}

impl std::hash::Hash for Script {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_reader().as_slice().hash(state)
    }
}
