use anyhow::Result;
use gw_common::H256;
use gw_mem_pool::traits::MemPoolErrorTxHandler;
use gw_runtime::spawn;
use gw_types::offchain::ErrorTxReceipt;
use rust_decimal::Decimal;
use sqlx::PgPool;
use tokio::task::JoinHandle;

use crate::helper::{hex, parse_log, GwLog};

pub const MAX_RETURN_DATA: usize = 96;
pub const MAX_ERROR_TX_RECEIPT_BLOCKS: u64 = 3;

pub struct ErrorReceiptIndexer {
    pool: PgPool,
    latest_block: u64,
}

impl ErrorReceiptIndexer {
    pub fn new(pool: PgPool) -> Self {
        ErrorReceiptIndexer {
            pool,
            latest_block: 0,
        }
    }

    async fn insert_error_tx_receipt(pool: PgPool, receipt: ErrorTxReceipt) -> Result<()> {
        let record = ErrorReceiptRecord::from(receipt);
        log::debug!("error tx receipt record {:?}", record);

        let mut db = pool.begin().await?;
        sqlx::query("INSERT INTO error_transactions (hash, block_number, cumulative_gas_used, gas_used, status_code, status_reason) VALUES ($1, $2, $3, $4, $5, $6)")
            .bind(hex(record.tx_hash.as_slice())?)
            .bind(Decimal::from(record.block_number))
            .bind(Decimal::from(record.cumulative_gas_used))
            .bind(Decimal::from(record.gas_used))
            .bind(Decimal::from(record.status_code))
            .bind(record.status_reason)
            .execute(&mut db)
            .await?;

        db.commit().await?;
        Ok(())
    }

    async fn clear_expired_block_error_receipt(pool: PgPool, block_number: u64) -> Result<()> {
        let mut db = pool.begin().await?;
        let result = sqlx::query("DELETE FROM error_transactions WHERE block_number <= $1")
            .bind(Decimal::from(block_number))
            .execute(&mut db)
            .await?;

        db.commit().await?;
        log::info!("delete error tx receipt {}", result.rows_affected());

        Ok(())
    }
}

impl MemPoolErrorTxHandler for ErrorReceiptIndexer {
    fn handle_error_receipt(&mut self, receipt: ErrorTxReceipt) -> JoinHandle<Result<()>> {
        if self.latest_block < receipt.block_number {
            self.latest_block = receipt.block_number;

            let pool = self.pool.clone();
            let expired_block = self
                .latest_block
                .saturating_sub(MAX_ERROR_TX_RECEIPT_BLOCKS);
            spawn(async move {
                if let Err(err) = Self::clear_expired_block_error_receipt(pool, expired_block).await
                {
                    log::error!("clear expired block error receipt {}", err);
                }
            });
        }

        let pool = self.pool.clone();
        spawn(async move {
            if let Err(err) = Self::insert_error_tx_receipt(pool, receipt).await {
                log::error!("insert error tx receipt {}", err);
            }
            Ok(())
        })
    }
}

#[derive(Debug)]
struct ErrorReceiptRecord {
    tx_hash: H256,
    block_number: u64,
    cumulative_gas_used: u64,
    gas_used: u64,
    status_code: u32,
    status_reason: Vec<u8>,
}

impl From<ErrorTxReceipt> for ErrorReceiptRecord {
    fn from(receipt: ErrorTxReceipt) -> Self {
        let return_data_len = std::cmp::min(receipt.return_data.len(), MAX_RETURN_DATA);
        let basic_record = ErrorReceiptRecord {
            tx_hash: receipt.tx_hash,
            block_number: receipt.block_number,
            cumulative_gas_used: 0,
            gas_used: 0,
            status_code: 0,
            status_reason: receipt.return_data[..return_data_len].to_vec(),
        };

        let gw_log = match receipt.last_log.map(|log| parse_log(&log)).transpose() {
            Ok(Some(log)) => log,
            Err(err) => {
                log::error!("[error receipt]: parse log error {}", err);
                return basic_record;
            }
            _ => return basic_record,
        };

        match gw_log {
            GwLog::PolyjuiceSystem {
                gas_used,
                cumulative_gas_used,
                created_address: _,
                status_code,
            } => {
                let isnt_string = |t: &ethabi::token::Token| -> bool {
                    !matches!(t, ethabi::token::Token::String(_))
                };

                // First 4 bytes are func signature
                let data = if receipt.return_data.len() > 4 {
                    &receipt.return_data[4..]
                } else {
                    &receipt.return_data[..]
                };
                let status_reason = match ethabi::decode(&[ethabi::ParamType::String], data) {
                    Ok(tokens) if !tokens.iter().any(isnt_string) => {
                        let mut reason = tokens
                            .into_iter()
                            .flat_map(ethabi::token::Token::into_string)
                            .collect::<Vec<String>>()
                            .join("");

                        reason.truncate(MAX_RETURN_DATA);
                        reason.as_bytes().to_vec()
                    }
                    _ => {
                        log::warn!("unsupported polyjuice reason {:?}", receipt.return_data);
                        basic_record.status_reason
                    }
                };

                ErrorReceiptRecord {
                    gas_used,
                    cumulative_gas_used,
                    status_code,
                    status_reason,
                    ..basic_record
                }
            }
            _ => basic_record,
        }
    }
}
