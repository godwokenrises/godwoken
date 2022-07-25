use crate::bytes::Bytes;
use crate::packed::{CellOutput, LogItem, Script};
use crate::prelude::*;
use sparse_merkle_tree::H256;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct RecoverAccount {
    pub message: H256,
    pub signature: Vec<u8>,
    pub lock_script: Script,
}
#[derive(Debug, Clone, Default)]
pub struct RunResultWriteState {
    pub write_values: HashMap<H256, H256>,
    pub account_count: Option<u32>,
    pub new_scripts: HashMap<H256, Script>,
    pub write_data: HashMap<H256, Bytes>,
    // log data
    pub logs: Vec<LogItem>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunResultCycles {
    pub execution: u64,
    pub r#virtual: u64,
}

impl RunResultCycles {
    pub fn total(&self) -> u64 {
        self.execution.saturating_add(self.r#virtual)
    }
}

#[derive(Debug, Clone, Default)]
pub struct RunResult {
    pub read_values: HashMap<H256, H256>,
    pub return_data: Bytes,
    pub recover_accounts: HashSet<RecoverAccount>,
    pub get_scripts: HashMap<H256, Script>,
    // data hash -> data full size
    pub read_data: HashMap<H256, Bytes>,
    // used cycles
    pub used_cycles: u64,
    pub exit_code: i8,
    pub write: RunResultWriteState,
    pub cycles: RunResultCycles,
}

impl RunResult {
    // clear all write fields
    pub fn revert_write(&mut self) {
        self.write = Default::default();
    }
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
