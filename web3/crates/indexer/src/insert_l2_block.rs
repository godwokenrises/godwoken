use std::{convert::TryFrom, str::FromStr};

use anyhow::Result;
use futures::StreamExt;
use gw_types::U256;
use itertools::Itertools;
use rust_decimal::Decimal;
use sqlx::{
    postgres::PgRow,
    types::{
        chrono::{DateTime, Utc},
        BigDecimal,
    },
    Row,
};
use sqlx::{Postgres, QueryBuilder};

use crate::{
    pool::POOL_FOR_UPDATE,
    types::{Block, Log, Transaction, TransactionWithLogs},
};

extern crate num_cpus;

const INSERT_LOGS_BATCH_SIZE: usize = 5000;

pub struct DbBlock<'a> {
    number: Decimal,
    hash: &'a [u8],
    parent_hash: &'a [u8],
    gas_limit: BigDecimal,
    gas_used: BigDecimal,
    timestamp: DateTime<Utc>,
    miner: &'a [u8],
    size: Decimal,
}

impl<'a> TryFrom<&'a Block> for DbBlock<'a> {
    type Error = anyhow::Error;

    fn try_from(block: &'a Block) -> Result<Self, Self::Error> {
        let a = Self {
            number: Decimal::from(block.number),
            hash: block.hash.as_slice(),
            parent_hash: block.parent_hash.as_slice(),
            gas_limit: u128_to_big_decimal(&block.gas_limit)?,
            gas_used: u128_to_big_decimal(&block.gas_used)?,
            timestamp: block.timestamp,
            miner: block.miner.as_ref(),
            size: Decimal::from(block.size),
        };
        Ok(a)
    }
}

#[derive(Debug, Clone)]
pub struct DbTransaction {
    hash: Vec<u8>,
    eth_tx_hash: Vec<u8>,
    block_number: Decimal,
    block_hash: Vec<u8>,
    transaction_index: Decimal,
    from_address: Vec<u8>,
    to_address: Option<Vec<u8>>,
    value: BigDecimal,
    nonce: Decimal,
    gas_limit: BigDecimal,
    gas_price: BigDecimal,
    input: Vec<u8>,
    v: Decimal,
    r: Vec<u8>,
    s: Vec<u8>,
    cumulative_gas_used: BigDecimal,
    gas_used: BigDecimal,
    contract_address: Option<Vec<u8>>,
    exit_code: Decimal,
    chain_id: Option<Decimal>,
}

impl TryFrom<Transaction> for DbTransaction {
    type Error = anyhow::Error;

    fn try_from(tx: Transaction) -> Result<DbTransaction, Self::Error> {
        let web3_to_address = tx.to_address.map(|addr| addr.to_vec());
        let web3_contract_address = tx.contract_address.map(|addr| addr.to_vec());
        let db_transaction = Self {
            hash: tx.gw_tx_hash.as_slice().to_vec(),
            eth_tx_hash: tx.compute_eth_tx_hash().as_slice().to_vec(),
            block_number: tx.block_number.into(),
            block_hash: tx.block_hash.as_slice().to_vec(),
            transaction_index: tx.transaction_index.into(),
            from_address: tx.from_address.to_vec(),
            to_address: web3_to_address,
            value: u256_to_big_decimal(&tx.value)?,
            nonce: tx.nonce.into(),
            gas_limit: u128_to_big_decimal(&tx.gas_limit)?,
            gas_price: u128_to_big_decimal(&tx.gas_price)?,
            input: tx.data,
            v: tx.v.into(),
            r: tx.r.to_vec(),
            s: tx.s.to_vec(),
            cumulative_gas_used: u128_to_big_decimal(&tx.cumulative_gas_used)?,
            gas_used: u128_to_big_decimal(&tx.gas_used)?,
            contract_address: web3_contract_address,
            exit_code: tx.exit_code.into(),
            chain_id: tx.chain_id.map(|id| id.into()),
        };
        Ok(db_transaction)
    }
}

#[derive(Debug, Clone)]
pub struct DbLog {
    transaction_id: i64,
    transaction_hash: Vec<u8>,
    transaction_index: Decimal,
    block_number: Decimal,
    block_hash: Vec<u8>,
    address: Vec<u8>,
    data: Vec<u8>,
    log_index: Decimal,
    topics: Vec<Vec<u8>>,
}

impl DbLog {
    pub fn try_from_log(log: Log, transaction_id: i64) -> Result<DbLog> {
        let topics = log
            .topics
            .into_iter()
            .map(|t| t.as_slice().to_vec())
            .collect();

        let db_log = Self {
            transaction_id,
            transaction_hash: log.transaction_hash.as_slice().to_vec(),
            transaction_index: log.transaction_index.into(),
            block_number: log.block_number.into(),
            block_hash: log.block_hash.as_slice().to_vec(),
            address: log.address.to_vec(),
            data: log.data,
            log_index: log.log_index.into(),
            topics,
        };
        Ok(db_log)
    }
}

pub async fn insert_web3_block(
    web3_block: Block,
    pg_tx: &mut sqlx::Transaction<'_, Postgres>,
) -> Result<()> {
    let block = DbBlock::try_from(&web3_block)?;

    sqlx::query(
        "INSERT INTO blocks (number, hash, parent_hash, gas_limit, gas_used, timestamp, miner, size) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"
    )
        .bind(block.number)
        .bind(block.hash)
        .bind(block.parent_hash)
        .bind(block.gas_limit)
        .bind(block.gas_used)
        .bind(block.timestamp)
        .bind(block.miner)
        .bind(block.size)
        .execute(pg_tx)
        .await?;

    Ok(())
}

pub async fn insert_web3_txs_and_logs(
    web3_tx_with_logs_vec: Vec<TransactionWithLogs>,
    pg_tx: &mut sqlx::Transaction<'_, Postgres>,
) -> Result<(usize, usize)> {
    if web3_tx_with_logs_vec.is_empty() {
        return Ok((0, 0));
    }

    let mut txs = Vec::with_capacity(web3_tx_with_logs_vec.len());
    let mut logs = Vec::with_capacity(txs.len() * 2);
    for (i, web3_tx_with_logs) in web3_tx_with_logs_vec.into_iter().enumerate() {
        // Set transaction_id to txs' index
        let db_logs: Vec<DbLog> = web3_tx_with_logs
            .logs
            .into_iter()
            .map(|l| DbLog::try_from_log(l, i as i64))
            .collect::<Result<Vec<_>>>()?;
        let tx = DbTransaction::try_from(web3_tx_with_logs.tx)?;
        txs.push(tx);
        logs.extend(db_logs);
    }

    let logs_len = logs.len();
    let txs_len = txs.len();

    let mut txs_query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
                "INSERT INTO transactions
                (hash, eth_tx_hash, block_number, block_hash, transaction_index, from_address, to_address, value, nonce, gas_limit, gas_price, input, v, r, s, cumulative_gas_used, gas_used, contract_address, exit_code, chain_id) "
            );

    txs_query_builder
        .push_values(txs, |mut b, tx| {
            b.push_bind(tx.hash)
                .push_bind(tx.eth_tx_hash)
                .push_bind(tx.block_number)
                .push_bind(tx.block_hash)
                .push_bind(tx.transaction_index)
                .push_bind(tx.from_address)
                .push_bind(tx.to_address)
                .push_bind(tx.value)
                .push_bind(tx.nonce)
                .push_bind(tx.gas_limit)
                .push_bind(tx.gas_price)
                .push_bind(tx.input)
                .push_bind(tx.v)
                .push_bind(tx.r)
                .push_bind(tx.s)
                .push_bind(tx.cumulative_gas_used)
                .push_bind(tx.gas_used)
                .push_bind(tx.contract_address)
                .push_bind(tx.exit_code)
                .push_bind(tx.chain_id);
        })
        .push(" RETURNING id");

    let mut tx_ids: Vec<i64> = vec![];

    let query = txs_query_builder.build();
    let rows: Vec<PgRow> = query.fetch_all(&mut (*pg_tx)).await?;
    let mut ids = rows
        .iter()
        .map(|r| r.get::<i64, _>("id"))
        .collect::<Vec<i64>>();
    tx_ids.append(&mut ids);

    let mut queries = logs.into_iter().chunks(INSERT_LOGS_BATCH_SIZE).into_iter().map(|logs|{

    let mut logs_query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
                    "INSERT INTO logs
                    (transaction_id, transaction_hash, transaction_index, block_number, block_hash, address, data, log_index, topics)"
                );
    // Get transaction id from preview insert returning
    logs_query_builder.push_values(logs, |mut b, log| {
        // transaction_id in log is transaction_id_index now
        let transaction_id = tx_ids[log.transaction_id as usize];

        b.push_bind(transaction_id)
            .push_bind(log.transaction_hash)
            .push_bind(log.transaction_index)
            .push_bind(log.block_number)
            .push_bind(log.block_hash)
            .push_bind(log.address)
            .push_bind(log.data)
            .push_bind(log.log_index)
            .push_bind(log.topics);
    });
    logs_query_builder
    }).collect::<Vec<_>>();

    let mut results_stream = futures::stream::iter(queries.iter_mut())
        .map(|query| query.build().execute(&*POOL_FOR_UPDATE))
        .buffer_unordered(POOL_FOR_UPDATE.size() as usize);

    while let Some(r) = results_stream.next().await {
        r?;
    }

    Ok((txs_len, logs_len))
}

pub async fn update_web3_block(
    web3_block: Block,
    pg_tx: &mut sqlx::Transaction<'_, Postgres>,
) -> Result<()> {
    let block = DbBlock::try_from(&web3_block)?;

    sqlx::query(
        "UPDATE blocks SET hash = $1, parent_hash = $2, gas_limit = $3, gas_used = $4, timestamp = $5, miner = $6, size = $7 where number = $8"
    )
        .bind(block.hash)
        .bind(block.parent_hash)
        .bind(block.gas_limit)
        .bind(block.gas_used)
        .bind(block.timestamp)
        .bind(block.miner)
        .bind(block.size)
        .bind(block.number)
        .execute(pg_tx)
        .await?;

    Ok(())
}

pub async fn update_web3_txs_and_logs(
    web3_tx_with_logs_vec: Vec<TransactionWithLogs>,
    _pg_tx: &mut sqlx::Transaction<'_, Postgres>,
) -> Result<(usize, usize)> {
    if web3_tx_with_logs_vec.is_empty() {
        return Ok((0, 0));
    }

    let mut txs = Vec::with_capacity(web3_tx_with_logs_vec.len());
    let mut logs = Vec::with_capacity(txs.len() * 2);

    for (i, web3_tx_with_logs) in web3_tx_with_logs_vec.into_iter().enumerate() {
        let db_logs: Vec<DbLog> = web3_tx_with_logs
            .logs
            .into_iter()
            .map(|l| DbLog::try_from_log(l, i as i64))
            .collect::<Result<_>>()?;
        let tx = DbTransaction::try_from(web3_tx_with_logs.tx)?;
        txs.push(tx);
        logs.extend(db_logs);
    }

    let txs_len = txs.len();
    let logs_len = logs.len();

    futures::future::join_all(
        txs.into_iter().map(|tx| {
                sqlx::query(
                    "UPDATE transactions SET hash = $1, eth_tx_hash = $2, from_address = $3, to_address = $4, value = $5, nonce = $6, gas_limit = $7, gas_price = $8, input = $9, v = $10, r = $11, s = $12, cumulative_gas_used = $13, gas_used = $14, contract_address = $15, exit_code = $16, chain_id = $17 where block_number = $18 and transaction_index = $19"
                )
                        .bind(tx.hash)
                            .bind(tx.eth_tx_hash)
                            .bind(tx.from_address)
                            .bind(tx.to_address)
                            .bind(tx.value)
                            .bind(tx.nonce)
                            .bind(tx.gas_limit)
                            .bind(tx.gas_price)
                            .bind(tx.input)
                            .bind(tx.v)
                            .bind(tx.r)
                            .bind(tx.s)
                            .bind(tx.cumulative_gas_used)
                            .bind(tx.gas_used)
                            .bind(tx.contract_address)
                            .bind(tx.exit_code)
                            .bind(tx.chain_id)
                            .bind(tx.block_number)
                            .bind(tx.transaction_index)
                            .execute(&*POOL_FOR_UPDATE)
        })
    )
    .await
    .into_iter()
    .collect::<Result<Vec<_>, sqlx::Error>>()?;

    let mut queries = logs.into_iter().chunks(INSERT_LOGS_BATCH_SIZE).into_iter().map(|logs|{
        let mut logs_query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
                "UPDATE logs SET transaction_hash = data_table.transaction_hash, address = data_table.address, data = data_table.data, topics = data_table.topics FROM ( "
            );

        logs_query_builder.push_values(logs, |mut b, log| {
                b.push_bind(log.transaction_hash)
                    .push_bind(log.address)
                    .push_bind(log.data)
                    .push_bind(log.topics)
                    .push_bind(log.block_number)
                    .push_bind(log.log_index);
            })
            .push(" ) AS data_table(transaction_hash, address, data, topics, block_number, log_index) WHERE logs.block_number = data_table.block_number AND logs.log_index = data_table.log_index");

       logs_query_builder
    }).collect::<Vec<_>>();

    let mut results_stream = futures::stream::iter(queries.iter_mut())
        .map(|query| query.build().execute(&*POOL_FOR_UPDATE))
        .buffer_unordered(POOL_FOR_UPDATE.size() as usize);

    while let Some(r) = results_stream.next().await {
        r?;
    }

    Ok((txs_len, logs_len))
}

fn u128_to_big_decimal(value: &u128) -> Result<BigDecimal> {
    let result = BigDecimal::from_str(&value.to_string())?;
    Ok(result)
}

fn u256_to_big_decimal(value: &U256) -> Result<BigDecimal> {
    let result = BigDecimal::from_str(&value.to_string())?;
    Ok(result)
}
