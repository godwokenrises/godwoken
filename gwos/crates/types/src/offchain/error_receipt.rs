use crate::bytes::Bytes;
use crate::h256::H256;
use crate::packed::LogItem;

#[derive(Debug, Clone)]
pub struct ErrorTxReceipt {
    pub tx_hash: H256,
    pub block_number: u64,
    pub return_data: Bytes,
    pub last_log: Option<LogItem>,
    pub exit_code: i8,
}
