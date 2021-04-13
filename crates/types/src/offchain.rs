use crate::packed::LogItem;
use sparse_merkle_tree::H256;
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct RunResult {
    pub read_values: HashMap<H256, H256>,
    pub write_values: HashMap<H256, H256>,
    pub return_data: Vec<u8>,
    pub account_count: Option<u32>,
    pub new_scripts: HashMap<H256, Vec<u8>>,
    pub write_data: HashMap<H256, Vec<u8>>,
    // data hash -> data full size
    pub read_data: HashMap<H256, usize>,
    // log data
    pub logs: Vec<LogItem>,
}
