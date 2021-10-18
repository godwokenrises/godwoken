use crate::packed::LogItem;

use sparse_merkle_tree::H256;

pub struct ErrorTxReceipt {
    pub tx_hash: H256,
    pub block_number: u64,
    pub return_data: Vec<u8>,
    pub last_log: Option<LogItem>,
}
