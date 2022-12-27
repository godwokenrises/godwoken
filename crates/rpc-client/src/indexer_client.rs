#![allow(clippy::mutable_key_type)]

use std::collections::HashMap;

use crate::ckb_client::CkbClient;
use crate::indexer_types::{Cell, Order, Pagination, ScriptType, SearchKey, SearchKeyFilter, Tx};
use crate::utils::{TracingHttpClient, DEFAULT_QUERY_LIMIT};
use anyhow::{Context, Result};
use ckb_types::prelude::Entity;
use gw_jsonrpc_types::ckb_jsonrpc_types::{JsonBytes, Uint32};
use gw_types::core::Timepoint;
use gw_types::offchain::{CompatibleFinalizedTimepoint, CustodianStat, SUDTStat};
use gw_types::packed::CustodianLockArgs;
use gw_types::{packed::Script, prelude::*};
use jsonrpc_utils::rpc_client;
use tracing::instrument;

#[derive(Clone)]
pub struct CkbIndexerClient {
    inner: TracingHttpClient,
    // True when using standalone CKB indexer, false when using the new built in CKB indexer.
    is_standalone: bool,
}

#[rpc_client]
impl CkbIndexerClient {
    async fn get_tip(&self) -> Result<gw_jsonrpc_types::blockchain::NumberHash>;
    async fn get_indexer_tip(&self) -> Result<gw_jsonrpc_types::blockchain::NumberHash>;
    pub async fn get_cells(
        &self,
        search_key: &SearchKey,
        order: &Order,
        limit: Uint32,
        cursor: &Option<JsonBytes>,
    ) -> Result<Pagination<Cell>>;
    pub async fn get_transactions(
        &self,
        search_key: &SearchKey,
        order: &Order,
        limit: Uint32,
        cursor: &Option<JsonBytes>,
    ) -> Result<Pagination<Tx>>;
}

impl From<CkbClient> for CkbIndexerClient {
    fn from(c: CkbClient) -> Self {
        Self {
            inner: c.inner,
            is_standalone: false,
        }
    }
}

impl CkbIndexerClient {
    /// Create a new CKBIndexerClient with standalone indexer url.
    pub fn with_url(url: &str) -> Result<Self> {
        let inner = TracingHttpClient::with_url(url.into())?;
        Ok(Self {
            inner,
            is_standalone: true,
        })
    }

    pub async fn get_indexer_tip1(&self) -> Result<gw_jsonrpc_types::blockchain::NumberHash> {
        if self.is_standalone {
            self.get_tip().await
        } else {
            self.get_indexer_tip().await
        }
    }

    #[instrument(skip_all, err(Debug), fields(timepoint = ?compatible_finalized_timepoint))]
    pub async fn stat_custodian_cells(
        &self,
        lock: Script,
        min_capacity: Option<u64>,
        compatible_finalized_timepoint: &CompatibleFinalizedTimepoint,
    ) -> Result<CustodianStat> {
        let mut sudt_stat: HashMap<ckb_types::packed::Script, SUDTStat> = HashMap::default();

        let filter = min_capacity.map(|min_capacity| SearchKeyFilter {
            output_capacity_range: Some([min_capacity.into(), u64::MAX.into()]),
            script: None,
            block_range: None,
            output_data_len_range: None,
        });
        let search_key = SearchKey {
            script: {
                let lock = ckb_types::packed::Script::new_unchecked(lock.as_bytes());
                lock.into()
            },
            script_type: ScriptType::Lock,
            filter,
        };
        let order = Order::Desc;
        let limit = Uint32::from(DEFAULT_QUERY_LIMIT as u32);

        let mut total_capacity = 0u128;
        let mut finalized_capacity = 0u128;
        let mut cells_count = 0;
        let mut ckb_cells_count = 0;
        let mut cursor = None;
        loop {
            let cells = self.get_cells(&search_key, &order, limit, &cursor).await?;
            if cells.last_cursor.is_empty() {
                break;
            }
            cursor = Some(cells.last_cursor);

            cells_count += cells.objects.len();
            for cell in cells.objects.into_iter() {
                let capacity: u64 = cell.output.capacity.into();
                total_capacity += capacity as u128;
                let is_finalized = {
                    let args = cell.output.lock.args.into_bytes();
                    let args = CustodianLockArgs::from_slice(&args[32..]).unwrap();
                    compatible_finalized_timepoint.is_finalized(&Timepoint::from_full_value(
                        args.deposit_finalized_timepoint().unpack(),
                    ))
                };
                if is_finalized {
                    finalized_capacity += capacity as u128;
                }

                if let Some(type_) = cell.output.type_.as_ref() {
                    assert_eq!(cell.output_data.len(), 16);

                    let type_: ckb_types::packed::Script = type_.to_owned().into();
                    let stat = sudt_stat.entry(type_).or_insert_with(Default::default);
                    let amount = {
                        let mut buf = [0u8; 16];
                        buf.copy_from_slice(cell.output_data.as_bytes());
                        u128::from_le_bytes(buf)
                    };
                    stat.total_amount += amount;
                    stat.cells_count += 1;
                    if is_finalized {
                        stat.finalized_amount += amount;
                    }
                } else {
                    ckb_cells_count += 1;
                }
            }
        }
        Ok(CustodianStat {
            cells_count,
            total_capacity,
            finalized_capacity,
            sudt_stat,
            ckb_cells_count,
        })
    }
}
