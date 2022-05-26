#![allow(clippy::mutable_key_type)]

use std::collections::HashMap;

use crate::error::RPCRequestError;
use crate::indexer_types::{Cell, Order, Pagination, ScriptType, SearchKey, SearchKeyFilter};
use crate::utils::{to_result, DEFAULT_HTTP_TIMEOUT, DEFAULT_QUERY_LIMIT};
use anyhow::Result;
use async_jsonrpc_client::{HttpClient, Params as ClientParams, Transport};
use ckb_types::prelude::Entity;
use gw_jsonrpc_types::ckb_jsonrpc_types::{JsonBytes, Uint32};
use gw_types::offchain::{CustodianStat, SUDTStat};
use gw_types::packed::CustodianLockArgs;
use gw_types::{packed::Script, prelude::*};
use serde::de::DeserializeOwned;
use serde_json::json;
use tracing::instrument;

#[derive(Clone)]
pub struct CKBIndexerClient(HttpClient);

impl CKBIndexerClient {
    pub fn new(ckb_indexer_client: HttpClient) -> Self {
        Self(ckb_indexer_client)
    }

    pub fn with_url(url: &str) -> Result<Self> {
        let client = HttpClient::builder()
            .timeout(DEFAULT_HTTP_TIMEOUT)
            .build(url)?;
        Ok(Self::new(client))
    }

    fn client(&self) -> &HttpClient {
        &self.0
    }

    #[instrument(skip_all, fields(method = method))]
    pub async fn request<T: DeserializeOwned>(
        &self,
        method: &str,
        params: Option<ClientParams>,
    ) -> Result<T> {
        let response =
            self.client().request(method, params).await.map_err(|err| {
                RPCRequestError::new("ckb indexer client", method.to_string(), err)
            })?;
        let response_str = response.to_string();
        match to_result(response) {
            Ok(r) => Ok(r),
            Err(err) => {
                log::error!(
                    "[ckb-indexer-client] Failed to parse response, method: {}, response: {}",
                    method,
                    response_str
                );
                Err(err)
            }
        }
    }

    pub async fn get_cells(
        &self,
        search_key: &SearchKey,
        order: &Order,
        limit: Option<Uint32>,
        cursor: &Option<JsonBytes>,
    ) -> Result<Pagination<Cell>> {
        self.request(
            "get_cells",
            Some(ClientParams::Array(vec![
                json!(search_key),
                json!(order),
                json!(limit.unwrap_or_else(|| (DEFAULT_QUERY_LIMIT as u32).into())),
                json!(cursor),
            ])),
        )
        .await
    }

    #[instrument(skip_all, fields(last_finalized_block_number = last_finalized_block_number))]
    pub async fn stat_custodian_cells(
        &self,
        lock: Script,
        min_capacity: Option<u64>,
        last_finalized_block_number: u64,
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
            let cells: Pagination<Cell> = self
                .request(
                    "get_cells",
                    Some(ClientParams::Array(vec![
                        json!(search_key),
                        json!(order),
                        json!(limit),
                        json!(cursor),
                    ])),
                )
                .await?;

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
                    args.deposit_block_number().unpack() <= last_finalized_block_number
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
