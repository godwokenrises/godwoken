use crate::bytes::Bytes;
use crate::packed::LogItem;

use sparse_merkle_tree::H256;

#[derive(Debug, Clone)]
pub struct ErrorTxReceipt {
    pub tx_hash: H256,
    pub block_number: u64,
    pub return_data: Bytes,
    pub last_log: Option<LogItem>,
    pub exit_code: i8,
}
