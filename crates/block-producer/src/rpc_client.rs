use crate::indexer_types::{Cell, Order, Pagination, ScriptType, SearchKey, SearchKeyFilter};
use crate::types::CellInfo;
use anyhow::Result;
use async_jsonrpc_client::{HttpClient, Output, Params as ClientParams, Transport};
use ckb_types::prelude::{Entity, Unpack as CKBUnpack};
use gw_common::H256;
use gw_generator::RollupContext;
use gw_jsonrpc_types::ckb_jsonrpc_types::{self, BlockNumber, Uint32};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{
        CellOutput, DepositionLockArgs, DepositionLockArgsReader, DepositionRequest, OutPoint,
        Script, Transaction,
    },
    prelude::*,
};
use serde::de::DeserializeOwned;
use serde_json::{from_value, json};

const DEFAULT_QUERY_LIMIT: usize = 1000;

#[derive(Debug, Clone)]
pub struct DepositInfo {
    pub request: DepositionRequest,
    pub cell: CellInfo,
}

fn to_result<T: DeserializeOwned>(output: Output) -> anyhow::Result<T> {
    match output {
        Output::Success(success) => Ok(from_value(success.result)?),
        Output::Failure(failure) => Err(anyhow::anyhow!("JSONRPC error: {}", failure.error)),
    }
}

fn parse_deposit_request(
    output: &CellOutput,
    output_data: &Bytes,
    deposit_lock_args: &DepositionLockArgs,
) -> Option<DepositionRequest> {
    let capacity = output.capacity();
    let script = deposit_lock_args.layer2_lock();
    let (sudt_script_hash, amount) = {
        match output.type_().to_opt() {
            Some(type_) => {
                let mut buf = [0u8; 16];
                if output_data.len() < buf.len() {
                    return None;
                }
                let len = buf.len();
                buf.copy_from_slice(&output_data[..len]);
                (type_.hash(), u128::from_le_bytes(buf))
            }
            None => ([0u8; 32], 0),
        }
    };

    let request = DepositionRequest::new_builder()
        .script(script)
        .capacity(capacity)
        .amount(amount.pack())
        .sudt_script_hash(sudt_script_hash.pack())
        .build();
    Some(request)
}

#[derive(Clone)]
pub struct RPCClient {
    pub indexer_client: HttpClient,
    pub ckb_client: HttpClient,
    pub rollup_type_script: ckb_types::packed::Script,
    pub rollup_context: RollupContext,
}

impl RPCClient {
    /// query lived rollup cell
    pub async fn query_rollup_cell(&self) -> Result<Option<CellInfo>> {
        let search_key = SearchKey {
            script: self.rollup_type_script.clone().into(),
            script_type: ScriptType::Type,
            filter: Some(SearchKeyFilter {
                script: None,
                output_data_len_range: None,
                output_capacity_range: None,
                block_range: None,
            }),
        };
        let order = Order::Desc;
        let limit = Uint32::from(1);

        let mut cells: Pagination<Cell> = to_result(
            self.indexer_client
                .request(
                    "get_cells",
                    Some(ClientParams::Array(vec![
                        json!(search_key),
                        json!(order),
                        json!(limit),
                    ])),
                )
                .await?,
        )?;
        if let Some(cell) = cells.objects.pop() {
            let out_point = {
                let out_point: ckb_types::packed::OutPoint = cell.out_point.into();
                OutPoint::new_unchecked(out_point.as_bytes())
            };
            let output = {
                let output: ckb_types::packed::CellOutput = cell.output.into();
                CellOutput::new_unchecked(output.as_bytes())
            };
            let data = cell.output_data.into_bytes();
            let cell_info = CellInfo {
                out_point,
                output,
                data,
            };
            return Ok(Some(cell_info));
        }
        Ok(None)
    }

    /// query payment cells, the returned cells should provide at least required_capacity fee,
    /// and the remained fees should be enough to cover a charge cell
    pub async fn query_payment_cells(
        &self,
        lock: Script,
        required_capacity: u64,
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
            let cells: Pagination<Cell> = to_result(
                self.indexer_client
                    .request(
                        "get_cells",
                        Some(ClientParams::Array(vec![
                            json!(search_key),
                            json!(order),
                            json!(limit),
                            json!(cursor),
                        ])),
                    )
                    .await?,
            )?;
            cursor = Some(cells.last_cursor);
            let cells = cells.objects.into_iter().filter_map(|cell| {
                // delete cells with data & type
                if !cell.output_data.is_empty() || cell.output.type_.is_some() {
                    return None;
                }
                let out_point = {
                    let out_point: ckb_types::packed::OutPoint = cell.out_point.into();
                    OutPoint::new_unchecked(out_point.as_bytes())
                };
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
        dbg!(&collected_cells, &collected_capacity);
        Ok(collected_cells)
    }

    pub async fn get_cell(&self, out_point: OutPoint) -> Result<Option<CellInfo>> {
        let json_out_point: ckb_jsonrpc_types::OutPoint = {
            let out_point = ckb_types::packed::OutPoint::new_unchecked(out_point.as_bytes());
            out_point.into()
        };
        let cell_with_status: Option<ckb_jsonrpc_types::CellWithStatus> = to_result(
            self.ckb_client
                .request(
                    "get_live_cell",
                    Some(ClientParams::Array(vec![
                        json!(json_out_point),
                        json!(true),
                    ])),
                )
                .await?,
        )?;
        let cell_info = cell_with_status.map(|cell_with_status| {
            let cell = cell_with_status.cell.expect("get cell");
            let output: ckb_types::packed::CellOutput = cell.output.into();
            let output = CellOutput::new_unchecked(output.as_bytes());
            let data = cell
                .data
                .map(|cell_data| cell_data.content.into_bytes())
                .unwrap_or_else(Bytes::new);
            let out_point = out_point.to_owned();
            CellInfo {
                output,
                data,
                out_point,
            }
        });
        Ok(cell_info)
    }

    async fn get_tip_block_number(&self) -> Result<u64> {
        let number: BlockNumber = to_result(
            self.ckb_client
                .request("get_tip_block_number", None)
                .await?,
        )?;
        Ok(number.value())
    }

    /// return all lived deposition requests
    pub async fn query_deposit_cells(&self) -> Result<Vec<DepositInfo>> {
        // search deposit cells from recent 10 blocks
        const SEARCH_BLOCKS: u64 = 10;

        let tip_number = self.get_tip_block_number().await?;
        let mut deposit_infos = Vec::new();
        for number in tip_number.saturating_sub(SEARCH_BLOCKS)..tip_number {
            let block_number = BlockNumber::from(number);

            let block: ckb_jsonrpc_types::BlockView = to_result(
                self.ckb_client
                    .request(
                        "get_block_by_number",
                        Some(ClientParams::Array(vec![json!(block_number)])),
                    )
                    .await?,
            )?;
            let txs: Vec<ckb_types::packed::Transaction> = block
                .transactions
                .into_iter()
                .map(|tx| tx.inner.into())
                .collect();
            for tx in txs {
                let tx_hash: [u8; 32] = tx.calc_tx_hash().unpack();
                for (index, (output, data)) in tx
                    .raw()
                    .outputs()
                    .into_iter()
                    .zip(tx.raw().outputs_data().into_iter())
                    .enumerate()
                {
                    let args: Bytes = output.lock().args().unpack();
                    let is_valid_deposit_cell = output.lock().code_hash().as_slice()
                        == self
                            .rollup_context
                            .rollup_config
                            .deposition_script_type_hash()
                            .as_slice()
                        && output.lock().hash_type() == ScriptHashType::Type.into()
                        && args.len() >= 32
                        && &args[..32] == self.rollup_context.rollup_script_hash.as_slice();
                    if !is_valid_deposit_cell {
                        continue;
                    }

                    // TODO check cell liveness

                    // we only allow sUDT as type script
                    if let Some(script) = output.type_().to_opt() {
                        let is_valid_sudt = script.code_hash().as_slice()
                            == self
                                .rollup_context
                                .rollup_config
                                .l1_sudt_script_type_hash()
                                .as_slice()
                            && script.hash_type() == ScriptHashType::Type.into();
                        if !is_valid_sudt {
                            continue;
                        }
                    }

                    let output = CellOutput::new_unchecked(output.as_bytes());
                    let data = data.unpack();
                    let out_point = OutPoint::new_builder()
                        .tx_hash(tx_hash.pack())
                        .index((index as u32).pack())
                        .build();
                    let cell = CellInfo {
                        out_point,
                        output,
                        data,
                    };
                    let deposit_lock_args =
                        match DepositionLockArgsReader::verify(&args[32..], false) {
                            Ok(()) => DepositionLockArgs::new_unchecked(args.slice(32..)),
                            Err(_) => {
                                eprintln!("invalid deposit cell args: \n{:?}", args);
                                continue;
                            }
                        };
                    let request =
                        match parse_deposit_request(&cell.output, &cell.data, &deposit_lock_args) {
                            Some(r) => r,
                            None => {
                                eprintln!("invalid deposit cell: \n{:?}", cell);
                                continue;
                            }
                        };
                    let info = DepositInfo { cell, request };
                    deposit_infos.push(info);
                }
            }
        }

        Ok(deposit_infos)
    }

    pub async fn get_transaction(&self, tx_hash: [u8; 32]) -> Result<Option<Transaction>> {
        let tx_hash: ckb_types::H256 = tx_hash.into();
        let tx_with_status: Option<ckb_jsonrpc_types::TransactionWithStatus> = to_result(
            self.ckb_client
                .request(
                    "get_transaction",
                    Some(ClientParams::Array(vec![json!(tx_hash)])),
                )
                .await?,
        )?;
        Ok(tx_with_status.map(|tx_with_status| {
            let tx: ckb_types::packed::Transaction = tx_with_status.transaction.inner.into();
            Transaction::new_unchecked(tx.as_bytes())
        }))
    }

    pub async fn send_transaction(&self, tx: Transaction) -> Result<H256> {
        let tx: ckb_jsonrpc_types::Transaction = {
            let tx = ckb_types::packed::Transaction::new_unchecked(tx.as_bytes());
            tx.into()
        };
        let tx_hash: ckb_types::H256 = to_result(
            self.ckb_client
                .request(
                    "send_transaction",
                    Some(ClientParams::Array(vec![json!(tx)])),
                )
                .await?,
        )?;
        let hash: [u8; 32] = tx_hash.into();
        Ok(hash.into())
    }
}
