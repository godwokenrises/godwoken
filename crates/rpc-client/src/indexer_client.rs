#![allow(clippy::mutable_key_type)]

use crate::error::RPCRequestError;
use crate::indexer_types::{Cell, Order, Pagination, ScriptType, SearchKey, SearchKeyFilter};
use crate::utils::{to_result, DEFAULT_HTTP_TIMEOUT, DEFAULT_QUERY_LIMIT};
use anyhow::{anyhow, Result};
use async_jsonrpc_client::{HttpClient, Params as ClientParams, Transport};
use ckb_types::prelude::Entity;
use gw_jsonrpc_types::ckb_jsonrpc_types::Uint32;
use gw_types::offchain::{CustodianStat, SUDTStat};
use gw_types::packed::CustodianLockArgs;
use gw_types::{
    offchain::CellInfo,
    packed::{CellOutput, OutPoint, Script},
    prelude::*,
};
use serde::de::DeserializeOwned;
use serde_json::json;

use std::collections::{HashMap, HashSet};

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

    /// query payment cells, the returned cells should provide at least required_capacity fee,
    /// and the remained fees should be enough to cover a charge cell
    pub async fn query_payment_cells(
        &self,
        lock: Script,
        required_capacity: u64,
        taken_outpoints: &HashSet<OutPoint>,
    ) -> Result<Vec<CellInfo>> {
        let search_key = SearchKey {
            script: {
                let lock = ckb_types::packed::Script::new_unchecked(lock.as_bytes());
                lock.into()
            },
            script_type: ScriptType::Lock,
            filter: None,
        };
        let order = Order::Desc;
        let limit = Uint32::from(DEFAULT_QUERY_LIMIT as u32);

        let mut collected_cells = Vec::new();
        let mut collected_capacity = 0u64;
        let mut cursor = None;
        while collected_capacity < required_capacity {
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
                return Err(anyhow!(
                    "no enough payment cells, required: {}, taken: {:?}",
                    required_capacity,
                    taken_outpoints
                ));
            }
            cursor = Some(cells.last_cursor);

            let cells = cells.objects.into_iter().filter_map(|cell| {
                let out_point = {
                    let out_point: ckb_types::packed::OutPoint = cell.out_point.into();
                    OutPoint::new_unchecked(out_point.as_bytes())
                };
                // delete cells with data & type
                if !cell.output_data.is_empty()
                    || cell.output.type_.is_some()
                    || taken_outpoints.contains(&out_point)
                {
                    return None;
                }
                let output = {
                    let output: ckb_types::packed::CellOutput = cell.output.into();
                    CellOutput::new_unchecked(output.as_bytes())
                };
                let data = cell.output_data.into_bytes();
                Some(CellInfo {
                    out_point,
                    output,
                    data,
                })
            });

            // collect least cells
            for cell in cells {
                collected_capacity =
                    collected_capacity.saturating_add(cell.output.capacity().unpack());
                collected_cells.push(cell);
                if collected_capacity >= required_capacity {
                    break;
                }
            }
        }
        Ok(collected_cells)
    }

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
