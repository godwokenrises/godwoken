use crate::bytes::Bytes;
use crate::packed::{CellOutput, LogItem, Script};
use crate::prelude::*;
use sparse_merkle_tree::H256;
use std::collections::HashSet;

use super::CycleMeter;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct RecoverAccount {
    pub message: H256,
    pub signature: Vec<u8>,
    pub lock_script: Script,
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
    pub return_data: Bytes,
    pub logs: Vec<LogItem>,
    pub exit_code: i8,
    pub cycles: CycleMeter,
    pub read_data_hashes: HashSet<H256>,
    pub write_data_hashes: HashSet<H256>,
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
