use rust_decimal::Decimal;
use sqlx::types::chrono::{DateTime, Utc};
#[derive(Debug)]
pub struct Block {
    pub number: Decimal,
    pub hash: String,
    pub parent_hash: String,
    pub logs_bloom: String,
    pub gas_limit: Decimal,
    pub gas_used: Decimal,
    pub miner: String,
    pub size: Decimal,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug)]
pub struct Transaction {
    pub hash: String,
    pub block_number: Decimal,
    pub block_hash: String,
    pub transaction_index: i32,
    pub from_address: String,
    pub to_address: Option<String>,
    pub value: Decimal,
    pub nonce: Decimal,
    pub gas_limit: Decimal,
    pub gas_price: Decimal,
    pub input: Option<String>,
    pub v: String,
    pub r: String,
    pub s: String,
    pub cumulative_gas_used: Decimal,
    pub gas_used: Decimal,
    pub logs_bloom: String,
    pub contract_address: Option<String>,
    pub status: bool,
}

impl Transaction {
    #[allow(clippy::clippy::too_many_arguments)]
    pub fn new(
        hash: String,
        block_number: Decimal,
        block_hash: String,
        transaction_index: i32,
        from_address: String,
        to_address: Option<String>,
        value: Decimal,
        nonce: Decimal,
        gas_limit: Decimal,
        gas_price: Decimal,
        input: Option<String>,
        v: String,
        r: String,
        s: String,
        cumulative_gas_used: Decimal,
        gas_used: Decimal,
        logs_bloom: String,
        contract_address: Option<String>,
        status: bool,
    ) -> Transaction {
        Transaction {
            hash,
            block_number,
            block_hash,
            transaction_index,
            from_address,
            to_address,
            value,
            nonce,
            gas_limit,
            gas_price,
            input,
            v,
            r,
            s,
            cumulative_gas_used,
            gas_used,
            logs_bloom,
            contract_address,
            status,
        }
    }
}

#[derive(Debug)]
pub struct Log {
    pub transaction_hash: String,
    pub transaction_index: i32,
    pub block_number: Decimal,
    pub block_hash: String,
    pub address: String,
    pub data: String,
    pub log_index: i32,
    pub topics: Vec<String>,
}

impl Log {
    #[allow(clippy::clippy::too_many_arguments)]
    pub fn new(
        transaction_hash: String,
        transaction_index: i32,
        block_number: Decimal,
        block_hash: String,
        address: String,
        data: String,
        log_index: i32,
        topics: Vec<String>,
    ) -> Log {
        Log {
            transaction_hash,
            transaction_index,
            block_number,
            block_hash,
            address,
            data,
            log_index,
            topics,
        }
    }
}

#[derive(Debug)]
pub struct TransactionWithLogs {
    pub tx: Transaction,
    pub logs: Vec<Log>,
}
