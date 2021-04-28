use crate::indexer_types::{Cell, Order, Pagination, ScriptType, SearchKey, SearchKeyFilter};
use crate::types::CellInfo;
use anyhow::{anyhow, Result};
use async_jsonrpc_client::{HttpClient, Output, Params as ClientParams, Transport};
use ckb_types::prelude::Entity;
use gw_common::H256;
use gw_generator::RollupContext;
use gw_jsonrpc_types::ckb_jsonrpc_types::{self, BlockNumber, Uint32};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{
        Block, CellOutput, DepositionLockArgs, DepositionLockArgsReader, DepositionRequest,
        OutPoint, Script, StakeLockArgs, StakeLockArgsReader, Transaction,
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

            if cells.last_cursor.is_empty() {
                return Err(anyhow!("no enough payment cells"));
            }
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

    pub async fn get_block_by_number(&self, number: u64) -> Result<Block> {
        let block_number = BlockNumber::from(number);
        let block: ckb_jsonrpc_types::BlockView = to_result(
            self.ckb_client
                .request(
                    "get_block_by_number",
                    Some(ClientParams::Array(vec![json!(block_number)])),
                )
                .await?,
        )?;
        let block: ckb_types::core::BlockView = block.into();
        Ok(Block::new_unchecked(block.data().as_bytes()))
    }

    /// return all lived deposition requests
    pub async fn query_deposit_cells(&self) -> Result<Vec<DepositInfo>> {
        let tip_l1_block_number: u64 = self.get_tip_block_number().await?;

        let mut deposit_infos = Vec::new();

        let rollup_type_hash: Bytes = self
            .rollup_context
            .rollup_script_hash
            .as_slice()
            .to_vec()
            .into();

        let script = Script::new_builder()
            .args(rollup_type_hash.pack())
            .code_hash(
                self.rollup_context
                    .rollup_config
                    .deposition_script_type_hash(),
            )
            .hash_type(ScriptHashType::Type.into())
            .build();

        let script = {
            let lock = ckb_types::packed::Script::new_unchecked(script.as_bytes());
            lock.into()
        };

        let search_key = SearchKey {
            script,
            script_type: ScriptType::Lock,
            filter: Some(SearchKeyFilter {
                script: None,
                output_data_len_range: None,
                output_capacity_range: None,
                block_range: Some([
                    BlockNumber::from(tip_l1_block_number.saturating_sub(100)),
                    BlockNumber::from(u64::max_value()),
                ]),
            }),
        };
        let order = Order::Asc;
        let limit = Uint32::from(100);

        let cells: Pagination<Cell> = to_result(
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

        let cells = cells.objects.into_iter().map(|cell| {
            let out_point = {
                let out_point: ckb_types::packed::OutPoint = cell.out_point.into();
                OutPoint::new_unchecked(out_point.as_bytes())
            };
            let output = {
                let output: ckb_types::packed::CellOutput = cell.output.into();
                CellOutput::new_unchecked(output.as_bytes())
            };
            let data = cell.output_data.into_bytes();
            CellInfo {
                out_point,
                output,
                data,
            }
        });

        for cell in cells {
            let args: Bytes = cell.output.lock().args().unpack();
            let deposit_lock_args = match DepositionLockArgsReader::verify(&args[32..], false) {
                Ok(()) => DepositionLockArgs::new_unchecked(args.slice(32..)),
                Err(_) => {
                    eprintln!("invalid deposit cell args: \n{:#x}", args);
                    continue;
                }
            };
            let request = match parse_deposit_request(&cell.output, &cell.data, &deposit_lock_args)
            {
                Some(r) => r,
                None => {
                    eprintln!("invalid deposit cell: \n{:?}", cell);
                    continue;
                }
            };

            let script = request.script();
            if script.hash_type() != ScriptHashType::Type.into() {
                eprintln!("Invalid deposit: unexpected hash_type: Data");
                continue;
            }
            if self
                .rollup_context
                .rollup_config
                .allowed_eoa_type_hashes()
                .into_iter()
                .all(|type_hash| script.code_hash() != type_hash)
            {
                eprintln!(
                    "Invalid deposit: unknown code_hash: {:?}",
                    hex::encode(script.code_hash().as_slice())
                );
                continue;
            }
            let args: Bytes = script.args().unpack();
            if args.len() < 32 {
                eprintln!(
                    "Invalid deposit: expect rollup_type_hash in the args but args is too short, len: {}",
                    args.len()
                );
                continue;
            }
            if &args[..32] != self.rollup_context.rollup_script_hash.as_slice() {
                eprintln!(
                    "Invalid deposit: rollup_type_hash mismatch, rollup_script_hash: {}, args[..32]: {}",
                    hex::encode(self.rollup_context.rollup_script_hash.as_slice()),
                    hex::encode(&args[..32]),
                );
                continue;
            }

            let info = DepositInfo { cell, request };
            deposit_infos.push(info);
        }

        Ok(deposit_infos)
    }

    pub async fn query_unlocked_stake(
        &self,
        rollup_context: &RollupContext,
        owner_lock_hash: [u8; 32],
        last_finalized_block_number: u64,
    ) -> Result<Option<CellInfo>> {
        let lock = Script::new_builder()
            .code_hash(rollup_context.rollup_config.stake_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(rollup_context.rollup_script_hash.as_slice().pack())
            .build();

        let search_key = SearchKey {
            script: {
                let lock = ckb_types::packed::Script::new_unchecked(lock.as_bytes());
                lock.into()
            },
            script_type: ScriptType::Lock,
            filter: Some(SearchKeyFilter {
                script: None,
                output_data_len_range: None,
                output_capacity_range: None,
                block_range: None,
            }),
        };
        let order = Order::Desc;
        let limit = Uint32::from(DEFAULT_QUERY_LIMIT as u32);

        let mut unlocked_cell = None;
        let mut cursor = None;

        while unlocked_cell.is_none() {
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

            if cells.last_cursor.is_empty() {
                println!("no unlocked stake");
                return Ok(None);
            }
            cursor = Some(cells.last_cursor);

            unlocked_cell = cells.objects.into_iter().find(|cell| {
                let args = cell.output.lock.args.clone().into_bytes();
                let stake_lock_args = match StakeLockArgsReader::verify(&args[32..], false) {
                    Ok(()) => StakeLockArgs::new_unchecked(args.slice(32..)),
                    Err(_) => return false,
                };

                stake_lock_args.stake_block_number().unpack() <= last_finalized_block_number
                    && stake_lock_args.owner_lock_hash().as_slice() == owner_lock_hash
            });
        }

        let unlocked_cell_info = |cell: Cell| {
            let out_point = {
                let out_point: ckb_types::packed::OutPoint = cell.out_point.into();
                OutPoint::new_unchecked(out_point.as_bytes())
            };
            let output = {
                let output: ckb_types::packed::CellOutput = cell.output.into();
                CellOutput::new_unchecked(output.as_bytes())
            };
            let data = cell.output_data.into_bytes();

            CellInfo {
                out_point,
                output,
                data,
            }
        };

        Ok(unlocked_cell.map(unlocked_cell_info))
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
