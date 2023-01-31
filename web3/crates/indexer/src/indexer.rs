use std::{
    collections::{HashMap, HashSet},
    iter::FromIterator,
};

use crate::{
    helper::{hex, parse_log, GwLog, PolyjuiceArgs, GW_LOG_POLYJUICE_SYSTEM},
    insert_l2_block::{
        insert_web3_block, insert_web3_txs_and_logs, update_web3_block, update_web3_txs_and_logs,
    },
    pool::POOL,
    types::{
        Block as Web3Block, Log as Web3Log, Transaction as Web3Transaction,
        TransactionWithLogs as Web3TransactionWithLogs,
    },
};
use anyhow::{anyhow, Result};
use ckb_hash::blake2b_256;
use ckb_types::H256;
use futures::*;
use gw_common::{builtins::CKB_SUDT_ACCOUNT_ID, registry_address::RegistryAddress};
use gw_types::{
    bytes::Bytes,
    packed::{L2Block, L2Transaction, SUDTArgs, SUDTArgsUnion, Script, TxReceipt},
    prelude::Unpack as GwUnpack,
    prelude::*,
    U256,
};
use gw_web3_rpc_client::{
    convertion, godwoken_async_client::GodwokenAsyncClient, godwoken_rpc_client::GodwokenRpcClient,
};
use itertools::Itertools;
use rust_decimal::{prelude::ToPrimitive, Decimal};
use sqlx::types::chrono::{DateTime, NaiveDateTime, Utc};

const MILLIS_PER_SEC: u64 = 1_000;
const TX_BATCH_SIZE: usize = 100;

pub struct Web3Indexer {
    l2_sudt_type_script_hash: H256,
    polyjuice_type_script_hash: H256,
    rollup_type_hash: H256,
    allowed_eoa_hashes: HashSet<H256>,
    godwoken_rpc_client: GodwokenRpcClient,
    godwoken_async_client: GodwokenAsyncClient,
}

impl Web3Indexer {
    pub fn new(
        l2_sudt_type_script_hash: H256,
        polyjuice_type_script_hash: H256,
        rollup_type_hash: H256,
        eth_account_lock_hash: H256,
        gw_rpc_url: &str,
    ) -> Self {
        let mut allowed_eoa_hashes = HashSet::default();
        allowed_eoa_hashes.insert(eth_account_lock_hash);
        let godwoken_rpc_client = GodwokenRpcClient::new(gw_rpc_url);
        let godwoken_async_client = GodwokenAsyncClient::with_url(gw_rpc_url).unwrap(); // TODO:

        Web3Indexer {
            l2_sudt_type_script_hash,
            polyjuice_type_script_hash,
            rollup_type_hash,
            allowed_eoa_hashes,
            godwoken_rpc_client,
            godwoken_async_client,
        }
    }

    pub async fn update_l2_block(&self, l2_block: L2Block) -> Result<(usize, usize)> {
        let number: u64 = l2_block.raw().number().unpack();
        // update block
        let (txs_len, logs_len) = self.insert_or_update_l2block(l2_block, true).await?;
        log::debug!(
            "web3 indexer: update block #{}, {} txs, {} logs",
            number,
            txs_len,
            logs_len
        );
        Ok((txs_len, logs_len))
    }

    pub async fn store_l2_block(&self, l2_block: L2Block) -> Result<(usize, usize)> {
        let number: u64 = l2_block.raw().number().unpack();
        let local_tip_number = self.tip_number().await?.unwrap_or(0);
        let mut txs_len = 0;
        let mut logs_len = 0;
        if number > local_tip_number || self.query_number(number).await?.is_none() {
            // insert l2 block
            (txs_len, logs_len) = self.insert_or_update_l2block(l2_block, false).await?;
            log::debug!(
                "web3 indexer: sync new block #{}, {} txs, {} logs",
                number,
                txs_len,
                logs_len
            );
        }
        Ok((txs_len, logs_len))
    }

    async fn query_number(&self, number: u64) -> Result<Option<u64>> {
        let row: Option<(Decimal,)> = sqlx::query_as(&format!(
            "SELECT number FROM blocks WHERE number={} LIMIT 1",
            number
        ))
        .fetch_optional(&*POOL)
        .await?;
        Ok(row.and_then(|(n,)| n.to_u64()))
    }

    async fn tip_number(&self) -> Result<Option<u64>> {
        let row: Option<(Decimal,)> =
            sqlx::query_as("SELECT number FROM blocks ORDER BY number DESC LIMIT 1")
                .fetch_optional(&*POOL)
                .await?;
        Ok(row.and_then(|(n,)| n.to_u64()))
    }

    // NOTE: remember to update `tx_index`, `cumulative_gas_used`, `log.transaction_index`
    fn filter_single_transaction(
        &self,
        l2_transaction: L2Transaction,
        block_number: u64,
        block_hash: gw_types::h256::H256,
        id_script_map: &std::collections::HashMap<u32, Option<Script>>,
    ) -> Result<Option<Web3TransactionWithLogs>> {
        let gw_tx_hash: gw_types::h256::H256 = l2_transaction.hash();
        let from_id: u32 = l2_transaction.raw().from_id().unpack();

        let mock_tx_index: u32 = 0;

        let from_script = id_script_map
            .get(&from_id)
            .ok_or_else(|| anyhow!("Can't get script by id in hashmap: {:?}", from_id))?
            .as_ref()
            .ok_or_else(|| anyhow!("Can't get script by id: {:?}", from_id))?;

        let from_script_code_hash: H256 = from_script.code_hash().unpack();
        // skip tx not in the allowed eoa account lock
        if !self.allowed_eoa_hashes.contains(&from_script_code_hash) {
            // continue;
            return Ok(None);
        }
        // from_address is the script's args in eth account lock
        let from_script_args = from_script.args().raw_data();
        if from_script_args.len() != 52 && from_script_args[0..32] == self.rollup_type_hash.0 {
            return Err(anyhow!(
                "Wrong from_address's script args, from_script_args: {:?}",
                from_script_args
            ));
        }
        let from_address = {
            let mut buf = [0u8; 20];
            buf.copy_from_slice(&from_script_args[32..52]);
            buf
        };

        // extract to_id corresponding script, check code_hash is either polyjuice contract code_hash or sudt contract code_hash
        let to_id = l2_transaction.raw().to_id().unpack();
        let to_script = id_script_map
            .get(&to_id)
            .ok_or_else(|| anyhow!("Can't get script by id in hashmap: {:?}", to_id))?
            .as_ref()
            .ok_or_else(|| anyhow!("Can't get script by id: {:?}", to_id))?;

        // assume the signature is compatible if length is 65, otherwise return zero
        let signature: [u8; 65] = if l2_transaction.signature().len() == 65 {
            let signature: Bytes = l2_transaction.signature().unpack();
            let mut buf = [0u8; 65];
            buf.copy_from_slice(&signature);
            buf
        } else {
            [0u8; 65]
        };

        let r = {
            let mut buf = [0u8; 32];
            buf.copy_from_slice(&signature[0..32]);
            buf
        };
        let s = {
            let mut buf = [0u8; 32];
            buf.copy_from_slice(&signature[32..64]);
            buf
        };
        let v: u8 = signature[64];

        if to_script.code_hash().as_slice() == self.polyjuice_type_script_hash.0 {
            let l2_tx_args = l2_transaction.raw().args();
            let polyjuice_args = PolyjuiceArgs::decode(l2_tx_args.raw_data().as_ref())?;

            // For CREATE contracts, tx.to_address = null;
            // for native transfers, tx.to_address = last 20 bytes of polyjuice_args;
            // otherwise, tx.to_address equals to the eth_address of tx.to_id
            let (to_address, _polyjuice_chain_id) = if polyjuice_args.is_create {
                (None, to_id)
            } else if let Some(ref address_vec) = polyjuice_args.to_address_when_native_transfer {
                let mut address = [0u8; 20];
                address.copy_from_slice(&address_vec[..]);
                (Some(address), to_id)
            } else {
                let args: gw_types::bytes::Bytes = to_script.args().unpack();
                let address = {
                    let mut to = [0u8; 20];
                    to.copy_from_slice(&args[36..]);
                    to
                };

                let polyjuice_chain_id = {
                    let mut data = [0u8; 4];
                    data.copy_from_slice(&args[32..36]);
                    u32::from_le_bytes(data)
                };
                (Some(address), polyjuice_chain_id)
            };
            let chain_id: u64 = l2_transaction.raw().chain_id().unpack();
            let nonce: u32 = l2_transaction.raw().nonce().unpack();
            let input = polyjuice_args.input.clone().unwrap_or_default();

            // read logs
            let tx_receipt: TxReceipt = self.get_transaction_receipt(gw_tx_hash, block_number)?;
            let log_item_vec = tx_receipt.logs();

            // read polyjuice system log
            let polyjuice_system_log_item = log_item_vec
                .clone()
                .into_iter()
                .find(|item| u8::from(item.service_flag()) == GW_LOG_POLYJUICE_SYSTEM);

            let (contract_address, tx_gas_used) = match polyjuice_system_log_item {
                Some(item) => {
                    let polyjuice_system_log = parse_log(&item, &gw_tx_hash)?;
                    if let GwLog::PolyjuiceSystem {
                        gas_used,
                        cumulative_gas_used: _,
                        created_address,
                        status_code: _,
                    } = polyjuice_system_log
                    {
                        let tx_gas_used: u128 = gas_used.into();
                        // cumulative_gas_used += tx_gas_used;
                        let contract_address =
                            if polyjuice_args.is_create && created_address != [0u8; 20] {
                                Some(created_address)
                            } else {
                                None
                            };
                        (contract_address, tx_gas_used)
                    } else {
                        return Err(anyhow!(
                            "can't find polyjuice system log from logs: tx_hash: {}",
                            hex(gw_tx_hash.as_slice())?
                        ));
                    }
                }
                None => {
                    let gw_tx_hash_hex = hex(gw_tx_hash.as_slice()).unwrap_or_else(|_| {
                        format!("Can't convert tx_hash: {:?} to hex format", gw_tx_hash)
                    });
                    log::error!(
                        "no system logs in tx_hash: {}, block_number: {}, exit_code: {}",
                        gw_tx_hash_hex,
                        block_number,
                        tx_receipt.exit_code()
                    );
                    (None, polyjuice_args.gas_limit as u128)
                }
            };

            let exit_code: u8 = tx_receipt.exit_code().into();
            let web3_transaction = Web3Transaction::new(
                gw_tx_hash,
                Some(chain_id),
                block_number,
                block_hash,
                mock_tx_index,
                from_address,
                to_address,
                polyjuice_args.value.into(),
                nonce,
                polyjuice_args.gas_limit.into(),
                polyjuice_args.gas_price,
                input,
                r,
                s,
                v,
                // cumulative_gas_used,
                0, // should update later
                tx_gas_used,
                contract_address,
                exit_code,
            );

            let web3_logs = {
                let mut logs: Vec<Web3Log> = vec![];
                // log_index is a log's index in block, not transaction, should update later.
                let mut log_index = 0;
                for log_item in log_item_vec {
                    let log = parse_log(&log_item, &gw_tx_hash)?;
                    match log {
                        GwLog::PolyjuiceSystem { .. } => {
                            // we already handled this
                        }
                        GwLog::PolyjuiceUser {
                            address,
                            data,
                            topics,
                        } => {
                            let web3_log = Web3Log::new(
                                gw_tx_hash,
                                mock_tx_index,
                                block_number,
                                block_hash,
                                address,
                                data,
                                log_index,
                                topics,
                            );
                            logs.push(web3_log);
                            log_index += 1;
                        }
                        // TODO: Given the fact that Ethereum doesn't emit event for native ether transfer at system level, the SudtTransfer/SudtPayFee logs in polyjuice provide more info than we need here and could be ignored so far.
                        GwLog::SudtTransfer { .. } => {}
                        GwLog::SudtPayFee { .. } => {}
                    }
                }
                logs
            };

            let web3_tx_with_logs = Web3TransactionWithLogs {
                tx: web3_transaction,
                logs: web3_logs,
            };
            // tx_index += 1;
            return Ok(Some(web3_tx_with_logs));
        } else if to_id == CKB_SUDT_ACCOUNT_ID
            && to_script.code_hash().as_slice() == self.l2_sudt_type_script_hash.0
        {
            // deal with SUDT transfer
            let sudt_args = SUDTArgs::from_slice(l2_transaction.raw().args().raw_data().as_ref())?;
            match sudt_args.to_enum() {
                SUDTArgsUnion::SUDTTransfer(sudt_transfer) => {
                    // Since we can transfer to any non-exists account, we can not check the script.code_hash.
                    let to_address_registry_address =
                        RegistryAddress::from_slice(sudt_transfer.to_address().as_slice());

                    let mut to_address = [0u8; 20];
                    if let Some(registry_address) = to_address_registry_address {
                        let address = registry_address.address;
                        if address.len() != 20 {
                            // continue;
                            return Ok(None);
                        }
                        to_address.copy_from_slice(address.as_slice());
                    } else {
                        // continue;
                        return Ok(None);
                    }

                    let amount: U256 = sudt_transfer.amount().unpack();
                    let fee: u128 = sudt_transfer.fee().amount().unpack();
                    let value = amount;

                    // Represent SUDTTransfer fee in web3 style, set gas_price as 1 temporary.
                    let gas_price = 1;
                    let gas_limit = fee;
                    // cumulative_gas_used += gas_limit;

                    let nonce: u32 = l2_transaction.raw().nonce().unpack();

                    let tx_receipt: TxReceipt =
                        self.get_transaction_receipt(gw_tx_hash, block_number)?;

                    let exit_code: u8 = tx_receipt.exit_code().into();
                    let web3_transaction = Web3Transaction::new(
                        gw_tx_hash,
                        None,
                        block_number,
                        block_hash,
                        mock_tx_index,
                        from_address,
                        Some(to_address),
                        value,
                        nonce,
                        gas_limit,
                        gas_price,
                        Vec::new(),
                        r,
                        s,
                        v,
                        0, // should update later
                        gas_limit,
                        None,
                        exit_code,
                    );

                    let web3_tx_with_logs = Web3TransactionWithLogs {
                        tx: web3_transaction,
                        logs: vec![],
                    };

                    return Ok(Some(web3_tx_with_logs));
                }
                SUDTArgsUnion::SUDTQuery(_sudt_query) => {}
            }
            // tx_index += 1;
        }
        Ok(None)
    }

    async fn batch_from_script(
        &self,
        txs: &[L2Transaction],
    ) -> Result<std::collections::HashMap<u32, Option<Script>>> {
        let from_ids = txs
            .iter()
            .map(|tx| {
                let from_id: u32 = tx.raw().from_id().unpack();
                from_id
            })
            .collect::<Vec<_>>();
        let from_ids_len = from_ids.len();

        let mut to_ids = txs
            .iter()
            .map(|tx| {
                let to_id: u32 = tx.raw().to_id().unpack();
                to_id
            })
            .collect::<Vec<_>>();
        let to_ids_len = to_ids.len();

        let mut ids = from_ids;
        ids.append(&mut to_ids);

        let id_sets: HashSet<u32> = std::collections::HashSet::from_iter(ids.into_iter());
        let ids = Vec::from_iter(id_sets.into_iter());
        let ids_len = ids.len();

        log::debug!(
            "batch_from_script request, from_id len: {}, to_id len: {}, id sets len: {}",
            from_ids_len,
            to_ids_len,
            ids_len
        );

        let scripts = batch_account_id_to_script(&self.godwoken_async_client, ids.clone()).await?;

        let mut hashmap = HashMap::<u32, Option<Script>>::new();
        scripts
            .into_iter()
            .zip(ids.into_iter())
            .for_each(|(value, id)| {
                hashmap.insert(id, value);
            });

        Ok(hashmap)
    }

    async fn insert_or_update_l2block(
        &self,
        l2_block: L2Block,
        is_update: bool,
    ) -> Result<(usize, usize)> {
        let block_number = l2_block.raw().number().unpack();
        let block_hash: gw_types::h256::H256 = blake2b_256(l2_block.raw().as_slice());
        // let mut cumulative_gas_used: u128 = 0;
        let l2_transactions = l2_block.transactions();
        let l2_transactions_vec: Vec<L2Transaction> = l2_transactions.into_iter().collect();

        let mut logs_len: usize = 0;
        let mut web3_txs_len: usize = 0;

        let id_script_hashmap = self.batch_from_script(&l2_transactions_vec).await?;

        let txs_slice = l2_transactions_vec
            .into_iter()
            .chunks(TX_BATCH_SIZE)
            .into_iter()
            .map(|chunk| chunk.collect())
            .collect::<Vec<Vec<_>>>();

        // begin db transaction
        let pool = &*POOL;
        let mut pg_tx = pool.begin().await?;

        let mut tx_index_cursor: u32 = 0;
        let mut log_index_cursor: u32 = 0;

        let mut cumulative_gas_used: u128 = 0;
        let mut total_gas_limit: u128 = 0;
        for txs in txs_slice {
            let l2_transaction_with_logs_vec = futures::stream::iter(txs.into_iter())
                .map(|tx| {
                    self.filter_single_transaction(tx, block_number, block_hash, &id_script_hashmap)
                })
                .collect::<Vec<Result<Option<Web3TransactionWithLogs>>>>()
                .await
                .into_iter()
                .collect::<Result<Vec<_>>>()?;

            let txs_vec = l2_transaction_with_logs_vec
                .into_iter()
                .flatten()
                .enumerate()
                .map(|(tx_index, mut tx)| {
                    let transaction_index = tx_index as u32 + tx_index_cursor;
                    tx.tx.transaction_index = transaction_index;
                    // update log.transaction_index too
                    // Update tx.log.index
                    tx.logs = tx
                        .logs
                        .into_iter()
                        .map(|mut log| {
                            log.transaction_index = transaction_index;
                            log.log_index += log_index_cursor;
                            log
                        })
                        .collect();
                    cumulative_gas_used += tx.tx.gas_used;
                    tx.tx.cumulative_gas_used = cumulative_gas_used;

                    total_gas_limit += tx.tx.gas_limit;
                    log_index_cursor += tx.logs.len() as u32;

                    tx
                })
                .collect::<Vec<_>>();

            tx_index_cursor += txs_vec.len() as u32;

            // insert to db or update
            let (txs_part_len, logs_part_len) = if is_update {
                update_web3_txs_and_logs(txs_vec, &mut pg_tx).await?
            } else {
                insert_web3_txs_and_logs(txs_vec, &mut pg_tx).await?
            };

            web3_txs_len += txs_part_len;
            logs_len += logs_part_len;
        }

        // insert or update block
        let web3_block = self
            .build_web3_block(&l2_block, total_gas_limit, cumulative_gas_used)
            .await?;
        if is_update {
            update_web3_block(web3_block, &mut pg_tx).await?;
        } else {
            insert_web3_block(web3_block, &mut pg_tx).await?;
        }

        // commit
        pg_tx.commit().await?;

        Ok((web3_txs_len, logs_len))
    }

    fn get_transaction_receipt(
        &self,
        gw_tx_hash: gw_types::h256::H256,
        block_number: u64,
    ) -> Result<TxReceipt> {
        let tx_hash = ckb_types::H256::from_slice(gw_tx_hash.as_slice())?;
        let tx_hash_hex = hex(tx_hash.as_bytes())
            .unwrap_or_else(|_| format!("convert tx hash: {:?} to hex format failed", tx_hash));

        let get_receipt = || -> Result<TxReceipt> {
            let tx_receipt: TxReceipt = self
                .godwoken_rpc_client
                .get_transaction_receipt(&tx_hash)?
                .ok_or_else(|| {
                    anyhow!(
                        "tx receipt not found by tx_hash: ({}) of block: {}",
                        tx_hash_hex,
                        block_number,
                    )
                })?
                .into();
            Ok(tx_receipt)
        };

        let max_retry = 10;
        let mut retry_times = 0;
        while retry_times < max_retry {
            let receipt = get_receipt();
            match receipt {
                Ok(tx_receipt) => return Ok(tx_receipt),
                Err(err) => {
                    log::error!("{}", err);
                    retry_times += 1;
                    // sleep and retry
                    let sleep_time = std::time::Duration::from_secs(retry_times);
                    std::thread::sleep(sleep_time);
                }
            }
        }
        get_receipt()
    }

    async fn build_web3_block(
        &self,
        l2_block: &L2Block,
        gas_limit: u128,
        gas_used: u128,
    ) -> Result<Web3Block> {
        let block_number = l2_block.raw().number().unpack();
        let block_hash: gw_types::h256::H256 = l2_block.hash();
        let parent_hash: gw_types::h256::H256 = l2_block.raw().parent_block_hash().unpack();
        let block_producer: Bytes = l2_block.raw().block_producer().unpack();
        let block_producer_registry_address = RegistryAddress::from_slice(&block_producer);

        // If registry_address is None, set miner address to zero-address
        let mut miner_address = [0u8; 20];
        if let Some(registry_address) = block_producer_registry_address {
            let address = registry_address.address;
            if address.is_empty() {
                log::warn!("Block producer address is empty");
            } else if address.len() != 20 {
                log::error!("Block producer address len not equal to 20: {:?}", address);
            } else {
                miner_address.copy_from_slice(address.as_slice());
            }
        } else {
            log::warn!("Block producer address is None");
        };

        let epoch_time_as_millis: u64 = l2_block.raw().timestamp().unpack();
        let timestamp =
            NaiveDateTime::from_timestamp((epoch_time_as_millis / MILLIS_PER_SEC) as i64, 0);
        let size = l2_block.raw().as_slice().len();
        let web3_block = Web3Block {
            number: block_number,
            hash: block_hash,
            parent_hash,
            gas_limit,
            gas_used,
            miner: miner_address,
            size,
            timestamp: DateTime::<Utc>::from_utc(timestamp, Utc),
        };
        Ok(web3_block)
    }
}

async fn batch_account_id_to_script(
    godwoken_async_client: &GodwokenAsyncClient,
    account_ids: Vec<u32>,
) -> Result<Vec<Option<Script>>> {
    if account_ids.is_empty() {
        return Ok(vec![]);
    }

    let script_hashes = godwoken_async_client
        .get_script_hash_batch(account_ids)
        .await?;
    let scripts = godwoken_async_client
        .get_script_batch(script_hashes)
        .await?;

    let result = scripts
        .into_iter()
        .map(|script| script.map(convertion::to_script))
        .collect::<Vec<_>>();

    Ok(result)
}
