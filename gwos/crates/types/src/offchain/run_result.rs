use crate::bytes::Bytes;
use crate::h256::H256;
use crate::packed::{LogItem, Script};
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
    pub debug_log_buf: Vec<u8>,
}
