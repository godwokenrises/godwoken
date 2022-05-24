#![allow(clippy::mutable_key_type)]

use crate::ckb_client::CKBClient;
use crate::indexer_client::CKBIndexerClient;
use crate::indexer_types::{Cell, Order, Pagination, ScriptType, SearchKey, SearchKeyFilter};
use crate::traits::IndexedCells;
use crate::utils::{to_h256, to_jsonh256, DEFAULT_QUERY_LIMIT, TYPE_ID_CODE_HASH};
use anyhow::{anyhow, Result};
use async_jsonrpc_client::Params as ClientParams;
use ckb_types::core::hardfork::HardForkSwitch;
use ckb_types::prelude::Entity;
use gw_common::{CKB_SUDT_SCRIPT_ARGS, H256};
use gw_jsonrpc_types::ckb_jsonrpc_types::{self, BlockNumber, Consensus, Uint32};
use gw_types::offchain::{
    CellStatus, CellWithStatus, CollectedCustodianCells, DepositInfo, RollupContext,
    WithdrawalsAmount,
};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    offchain::CellInfo,
    packed::{
        Block, CellOutput, CustodianLockArgs, CustodianLockArgsReader, DepositLockArgs,
        DepositLockArgsReader, DepositRequest, NumberHash, OutPoint, Script, StakeLockArgs,
        StakeLockArgsReader, Transaction, WithdrawalLockArgs, WithdrawalLockArgsReader,
    },
    prelude::*,
};
use rand::prelude::*;
use serde_json::json;
use tracing::instrument;

use std::time::Instant;
use std::{collections::HashSet, time::Duration};

fn to_cell_info(cell: Cell) -> CellInfo {
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
}

fn parse_deposit_request(
    output: &CellOutput,
    output_data: &Bytes,
    deposit_lock_args: &DepositLockArgs,
) -> Option<DepositRequest> {
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

    let request = DepositRequest::new_builder()
        .script(script)
        .capacity(capacity)
        .amount(amount.pack())
        .sudt_script_hash(sudt_script_hash.pack())
        .registry_id(deposit_lock_args.registry_id())
        .build();
    Some(request)
}

pub enum QueryResult<T> {
    Full(T),
    NotEnough(T),
}

impl<T> QueryResult<T> {
    pub fn expect_full(self, msg: &str) -> Result<T> {
        match self {
            Self::Full(r) => Ok(r),
            Self::NotEnough(_r) => Err(anyhow!(msg.to_string())),
        }
    }

    pub fn expect_any(self) -> T {
        match self {
            Self::Full(r) => r,
            Self::NotEnough(r) => r,
        }
    }

    pub fn map<R, F: FnOnce(T) -> R>(self, f: F) -> QueryResult<R> {
        match self {
            Self::Full(r) => QueryResult::Full(f(r)),
            Self::NotEnough(r) => QueryResult::NotEnough(f(r)),
        }
    }
}

#[derive(Clone)]
pub struct RPCClient {
    pub indexer: CKBIndexerClient,
    pub ckb: CKBClient,
    pub rollup_type_script: ckb_types::packed::Script,
    pub rollup_context: RollupContext,
}

impl RPCClient {
    pub fn new(
        rollup_type_script: ckb_types::packed::Script,
        rollup_context: RollupContext,
        ckb: CKBClient,
        indexer: CKBIndexerClient,
    ) -> Self {
        Self {
            indexer,
            ckb,
            rollup_context,
            rollup_type_script,
        }
    }

    /// query lived rollup cell
    #[instrument(skip_all)]
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

        let mut cells: Pagination<Cell> = self
            .indexer
            .request(
                "get_cells",
                Some(ClientParams::Array(vec![
                    json!(search_key),
                    json!(order),
                    json!(limit),
                ])),
            )
            .await?;
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

    /// this function queries identity cell by args
    #[instrument(skip_all)]
    pub async fn query_identity_cell(&self, args: Bytes) -> Result<Option<CellInfo>> {
        let search_key = SearchKey {
            script: ckb_types::packed::Script::new_builder()
                .code_hash(ckb_types::prelude::Pack::pack(&*TYPE_ID_CODE_HASH))
                .hash_type(ScriptHashType::Type.into())
                .args(ckb_types::prelude::Pack::pack(&args))
                .build()
                .into(),
            script_type: ScriptType::Type,
            filter: None,
        };
        let order = Order::Desc;
        let limit = Uint32::from(DEFAULT_QUERY_LIMIT as u32);

        let mut cell = None;
        let mut cursor = None;
        while cell.is_none() {
            let cells: Pagination<Cell> = self
                .indexer
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
            cursor = Some(cells.last_cursor);
            assert!(
                cells.objects.len() <= 1,
                "Never returns more than 1 identity cells"
            );
            cell = cells.objects.into_iter().find_map(|cell| {
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
        }
        Ok(cell)
    }

    /// this function return a cell that do not has data & _type fields
    #[instrument(skip_all)]
    pub async fn query_owner_cell(
        &self,
        lock: Script,
        filter_inputs: Option<HashSet<OutPoint>>,
    ) -> Result<Option<CellInfo>> {
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

        let mut cell = None;
        let mut cursor = None;
        while cell.is_none() {
            let cells: Pagination<Cell> = self
                .indexer
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
                return Err(anyhow!("no owner cell"));
            }
            cursor = Some(cells.last_cursor);

            cell = cells.objects.into_iter().find_map(|cell| {
                // delete cells with data & type
                if !cell.output_data.is_empty() || cell.output.type_.is_some() {
                    return None;
                }
                let out_point = {
                    let out_point: ckb_types::packed::OutPoint = cell.out_point.clone().into();
                    OutPoint::new_unchecked(out_point.as_bytes())
                };
                match filter_inputs {
                    Some(ref filter_inputs) if filter_inputs.contains(&out_point) => None,
                    _ => Some(to_cell_info(cell)),
                }
            });
        }
        Ok(cell)
    }

    #[instrument(skip_all, fields(tx_hash = %out_point.tx_hash(), index = Unpack::<u32>::unpack(&out_point.index())))]
    pub async fn get_cell(&self, out_point: OutPoint) -> Result<Option<CellWithStatus>> {
        let json_out_point: ckb_jsonrpc_types::OutPoint = {
            let out_point = ckb_types::packed::OutPoint::new_unchecked(out_point.as_bytes());
            out_point.into()
        };
        let cell_with_status: Option<gw_jsonrpc_types::ckb_jsonrpc_types::CellWithStatus> = self
            .ckb
            .request(
                "get_live_cell",
                Some(ClientParams::Array(vec![
                    json!(json_out_point),
                    json!(true),
                ])),
            )
            .await?;

        if cell_with_status.is_none() {
            return Ok(None);
        }
        let cell_with_status = cell_with_status.unwrap();
        let cell_info = cell_with_status.cell.map(|cell| {
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
        let status = match cell_with_status.status.as_str() {
            "live" => CellStatus::Live,
            "dead" => CellStatus::Dead,
            "unknown" => CellStatus::Unknown,
            err => return Err(anyhow!("can't parse cell status: {}", err)),
        };
        Ok(Some(CellWithStatus {
            cell: cell_info,
            status,
        }))
    }

    #[instrument(skip_all, fields(tx_hash = %out_point.tx_hash(), index = Unpack::<u32>::unpack(&out_point.index())))]
    pub async fn get_cell_from_mempool(&self, out_point: OutPoint) -> Result<Option<CellInfo>> {
        let tx = match self
            .ckb
            .get_transaction(out_point.tx_hash().unpack())
            .await?
        {
            Some(tx) => tx,
            None => return Ok(None),
        };

        let index: u32 = out_point.index().unpack();
        let raw_tx = tx.raw();

        let output: CellOutput = match raw_tx.outputs().get(index as usize) {
            Some(output) => output,
            None => return Ok(None),
        };
        let data = {
            let data = raw_tx.outputs_data().get(index as usize);
            data.map(|b| b.unpack()).unwrap_or_else(Bytes::new)
        };

        Ok(Some(CellInfo {
            output,
            data,
            out_point,
        }))
    }

    #[instrument(skip_all)]
    pub async fn get_tip(&self) -> Result<NumberHash> {
        let number_hash: gw_jsonrpc_types::blockchain::NumberHash =
            self.indexer.request("get_tip", None).await?;
        Ok(number_hash.into())
    }

    #[instrument(skip_all, fields(block_hash = %block_hash.pack()))]
    pub async fn get_block_median_time(&self, block_hash: H256) -> Result<Option<Duration>> {
        let opt_median_time: Option<gw_jsonrpc_types::ckb_jsonrpc_types::Uint64> = self
            .ckb
            .request(
                "get_block_median_time",
                Some(ClientParams::Array(vec![json!(to_jsonh256(block_hash))])),
            )
            .await?;

        Ok(opt_median_time.map(|t| Duration::from_millis(t.into())))
    }

    #[instrument(skip_all, fields(block = number))]
    pub async fn get_block_by_number(&self, number: u64) -> Result<Option<Block>> {
        let block_number = BlockNumber::from(number);
        let block_opt: Option<ckb_jsonrpc_types::BlockView> = self
            .ckb
            .request(
                "get_block_by_number",
                Some(ClientParams::Array(vec![json!(block_number)])),
            )
            .await?;
        Ok(block_opt.map(|b| {
            let block: ckb_types::core::BlockView = b.into();
            Block::new_unchecked(block.data().as_bytes())
        }))
    }

    /// return all lived deposit requests
    /// NOTICE the returned cells may contains invalid cells.
    #[instrument(skip(self))]
    pub async fn query_deposit_cells(
        &self,
        count: usize,
        min_ckb_deposit_capacity: u64,
        min_sudt_deposit_capacity: u64,
    ) -> Result<Vec<DepositInfo>> {
        const BLOCKS_TO_SEARCH: u64 = 2000;

        let tip_number = self.get_tip().await?.number().unpack();
        let mut deposit_infos = Vec::new();

        let rollup_type_hash: Bytes = self
            .rollup_context
            .rollup_script_hash
            .as_slice()
            .to_vec()
            .into();

        let script = Script::new_builder()
            .args(rollup_type_hash.pack())
            .code_hash(self.rollup_context.rollup_config.deposit_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .build();

        let script = {
            let lock = ckb_types::packed::Script::new_unchecked(script.as_bytes());
            lock.into()
        };
        let from_block = tip_number.saturating_sub(BLOCKS_TO_SEARCH);
        let to_block = u64::max_value();

        log::debug!(target: "collect-deposit-cells", "start searching deposit cells from_block {} to_block {} count {} min_ckb_deposit_capacity {} min_sudt_deposit_capacity {}",
             from_block, to_block, count, min_ckb_deposit_capacity, min_sudt_deposit_capacity);

        let search_key = SearchKey {
            script,
            script_type: ScriptType::Lock,
            filter: Some(SearchKeyFilter {
                script: None,
                output_data_len_range: None,
                output_capacity_range: Some([min_ckb_deposit_capacity.into(), u64::MAX.into()]),
                block_range: Some([BlockNumber::from(from_block), BlockNumber::from(to_block)]),
            }),
        };
        let order = Order::Asc;

        let mut cursor = None;

        while deposit_infos.len() < count {
            let limit = Uint32::from((count - deposit_infos.len()) as u32);

            let cells: Pagination<Cell> = self
                .indexer
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

            log::debug!(target: "collect-deposit-cells", "query {} cells", cells.objects.len());

            if cells.last_cursor.is_empty() {
                break;
            }
            cursor = Some(cells.last_cursor);

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
                // Ensure finalized ckb custodians are clearly mergeable
                if cell.output.type_().is_none() && !cell.data.is_empty() {
                    continue;
                }

                let args: Bytes = cell.output.lock().args().unpack();
                let deposit_lock_args = match DepositLockArgsReader::verify(&args[32..], false) {
                    Ok(()) => DepositLockArgs::new_unchecked(args.slice(32..)),
                    Err(_) => {
                        log::debug!(target: "collect-deposit-cells", "invalid deposit cell args: \n{:#x}", args);
                        continue;
                    }
                };
                let request = match parse_deposit_request(
                    &cell.output,
                    &cell.data,
                    &deposit_lock_args,
                ) {
                    Some(r) => r,
                    None => {
                        log::debug!(target: "collect-deposit-cells", "invalid deposit cell: \n{:?}", cell);
                        continue;
                    }
                };

                let cell_capacity = cell.output.capacity().unpack();
                if cell.output.type_().is_some() && cell_capacity < min_sudt_deposit_capacity {
                    log::debug!(
                        target: "collect-deposit-cells",
                        "invalid sudt deposit cell, required capacity: {}, capacity: {}",
                        min_sudt_deposit_capacity,
                        cell_capacity
                    );
                    continue;
                }

                let info = DepositInfo { cell, request };
                deposit_infos.push(info);
            }
        }

        log::debug!(target: "collect-deposit-cells", "return {} filtered cells", deposit_infos.len());

        Ok(deposit_infos)
    }

    /// query stake
    /// return cell which stake_block_number is less than last_finalized_block_number if the args isn't none
    /// otherwise return stake cell randomly
    #[instrument(
        skip_all,
        fields(required_staking_capacity, last_finalized_block_number)
    )]
    pub async fn query_stake(
        &self,
        rollup_context: &RollupContext,
        owner_lock_hash: [u8; 32],
        required_staking_capacity: u64,
        last_finalized_block_number: Option<u64>,
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
                output_capacity_range: Some([required_staking_capacity.into(), u64::MAX.into()]),
                block_range: None,
            }),
        };
        let order = Order::Desc;
        let limit = Uint32::from(DEFAULT_QUERY_LIMIT as u32);

        let mut stake_cell = None;
        let mut cursor = None;

        while stake_cell.is_none() {
            let cells: Pagination<Cell> = self
                .indexer
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
                log::debug!("no unlocked stake");
                return Ok(None);
            }
            cursor = Some(cells.last_cursor);

            stake_cell = cells.objects.into_iter().find(|cell| {
                let args = cell.output.lock.args.clone().into_bytes();
                let stake_lock_args = match StakeLockArgsReader::verify(&args[32..], false) {
                    Ok(()) => StakeLockArgs::new_unchecked(args.slice(32..)),
                    Err(_) => return false,
                };
                match last_finalized_block_number {
                    Some(last_finalized_block_number) => {
                        stake_lock_args.stake_block_number().unpack() <= last_finalized_block_number
                            && stake_lock_args.owner_lock_hash().as_slice() == owner_lock_hash
                    }
                    None => stake_lock_args.owner_lock_hash().as_slice() == owner_lock_hash,
                }
            });
        }

        let fetch_cell_info = |cell: Cell| {
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

        Ok(stake_cell.map(fetch_cell_info))
    }

    #[instrument(skip_all)]
    pub async fn query_stake_cells_by_owner_lock_hashes(
        &self,
        owner_lock_hashes: impl Iterator<Item = [u8; 32]>,
    ) -> Result<Vec<CellInfo>> {
        let lock = Script::new_builder()
            .code_hash(self.rollup_context.rollup_config.stake_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(self.rollup_context.rollup_script_hash.as_slice().pack())
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

        let owner_lock_hashes: HashSet<[u8; 32]> = owner_lock_hashes.collect();
        let mut collected_owners = HashSet::new();
        let mut collected_cells = Vec::new();
        let mut cursor = None;

        while collected_owners.len() != owner_lock_hashes.len() {
            let cells: Pagination<Cell> = self
                .indexer
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
                return Err(anyhow!("no all reward stake cells found"));
            }
            cursor = Some(cells.last_cursor);

            for cell in cells.objects.into_iter() {
                let args = cell.output.lock.args.clone().into_bytes();
                let stake_lock_args = match StakeLockArgsReader::verify(&args[32..], false) {
                    Ok(()) => StakeLockArgs::new_unchecked(args.slice(32..)),
                    Err(_) => continue,
                };

                let owner_lock_hash: [u8; 32] = stake_lock_args.owner_lock_hash().unpack();
                if owner_lock_hashes.contains(&owner_lock_hash)
                    && !collected_owners.contains(&owner_lock_hash)
                {
                    collected_owners.insert(owner_lock_hash);
                    collected_cells.push(to_cell_info(cell));
                }
            }
        }

        Ok(collected_cells)
    }

    #[instrument(skip(self))]
    pub async fn query_custodian_cells_by_block_hashes(
        &self,
        block_hashes: &HashSet<H256>,
    ) -> Result<(Vec<CellInfo>, HashSet<H256>)> {
        let rollup_context = &self.rollup_context;

        let custodian_lock = Script::new_builder()
            .code_hash(rollup_context.rollup_config.custodian_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(rollup_context.rollup_script_hash.as_slice().pack())
            .build();

        let search_key = SearchKey {
            script: ckb_types::packed::Script::new_unchecked(custodian_lock.as_bytes()).into(),
            script_type: ScriptType::Lock,
            filter: None,
        };
        let order = Order::Desc;
        let limit = Uint32::from(DEFAULT_QUERY_LIMIT as u32);

        let mut collected = vec![];
        let mut collected_block_hashes = HashSet::new();
        let mut cursor = None;

        while collected.is_empty() {
            let cells: Pagination<Cell> = self
                .indexer
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
                return Ok((collected, collected_block_hashes));
            }
            cursor = Some(cells.last_cursor);

            for cell in cells.objects.into_iter() {
                let args = cell.output.lock.args.clone().into_bytes();
                let custodian_lock_args = match CustodianLockArgsReader::verify(&args[32..], false)
                {
                    Ok(()) => CustodianLockArgs::new_unchecked(args.slice(32..)),
                    Err(_) => continue,
                };

                let deposit_block_hash: H256 = custodian_lock_args.deposit_block_hash().unpack();
                if !block_hashes.contains(&deposit_block_hash) {
                    continue;
                }

                collected.push(to_cell_info(cell));
                collected_block_hashes.insert(deposit_block_hash);
            }
        }

        Ok((collected, collected_block_hashes))
    }

    #[instrument(skip_all, fields(last_finalized_block_number = last_finalized_block_number, max_cells = max_cells))]
    pub async fn query_mergeable_ckb_custodians_cells(
        &self,
        mut collected: CollectedCustodianCells,
        last_finalized_block_number: u64,
        max_cells: usize,
    ) -> Result<QueryResult<CollectedCustodianCells>> {
        const MIN_MERGE_CELLS: usize = 5;
        log::debug!("ckb merge MIN_MERGE_CELLS {}", MIN_MERGE_CELLS);

        let remain = max_cells.saturating_sub(collected.cells_info.len());
        if remain < MIN_MERGE_CELLS {
            log::debug!("ckb merge break remain < `MIN_MERGE_CELLS`");
            return Ok(QueryResult::NotEnough(collected));
        }

        let rollup_context = &self.rollup_context;
        let custodian_lock = Script::new_builder()
            .code_hash(rollup_context.rollup_config.custodian_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(rollup_context.rollup_script_hash.as_slice().pack())
            .build();
        let filter = Some(SearchKeyFilter {
            script: None,
            block_range: None,
            output_data_len_range: Some([0.into(), 1.into()]), // [inclusive, exclusive]
            output_capacity_range: None,
        });
        let search_key = SearchKey {
            script: ckb_types::packed::Script::new_unchecked(custodian_lock.as_bytes()).into(),
            script_type: ScriptType::Lock,
            filter,
        };
        let order = Order::Desc;
        let limit = Uint32::from(DEFAULT_QUERY_LIMIT as u32);

        let mut collected_set: HashSet<_> = {
            let cells = collected.cells_info.iter();
            cells.map(|i| i.out_point.clone()).collect()
        };

        let mut cursor = None;
        let mut collected_ckb_custodians = Vec::<CellInfo>::with_capacity(remain);
        while collected_ckb_custodians.len() < remain {
            let cells: Pagination<Cell> = self
                .indexer
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

            for cell in cells.objects.into_iter() {
                if collected.cells_info.len() >= max_cells {
                    return Ok(QueryResult::Full(collected));
                }

                let info = to_cell_info(cell);
                if collected_set.contains(&info.out_point) {
                    continue;
                }

                let args: Bytes = info.output.lock().args().unpack();
                let custodian_lock_args = match CustodianLockArgsReader::verify(&args[32..], false)
                {
                    Ok(()) => CustodianLockArgs::new_unchecked(args.slice(32..)),
                    Err(_) => continue,
                };
                if custodian_lock_args.deposit_block_number().unpack() > last_finalized_block_number
                {
                    continue;
                }

                collected_set.insert(info.out_point.clone());
                collected_ckb_custodians.push(info);
            }

            if cells.last_cursor.is_empty() {
                break;
            }
            cursor = Some(cells.last_cursor);
        }

        if collected_ckb_custodians.len() < MIN_MERGE_CELLS {
            log::debug!("not enough `MIN_MERGE_CELLS` ckb custodians");
            return Ok(QueryResult::NotEnough(collected));
        }

        log::info!("merge ckb custodians {}", collected_ckb_custodians.len());
        for info in collected_ckb_custodians {
            collected.capacity = collected
                .capacity
                .saturating_add(info.output.capacity().unpack() as u128);
            collected.cells_info.push(info);
        }

        if collected.cells_info.len() < max_cells {
            Ok(QueryResult::NotEnough(collected))
        } else {
            Ok(QueryResult::Full(collected))
        }
    }

    #[instrument(skip_all, fields(last_finalized_block_number = last_finalized_block_number, max_cells = max_cells))]
    pub async fn query_mergeable_sudt_custodians_cells(
        &self,
        mut collected: CollectedCustodianCells,
        last_finalized_block_number: u64,
        max_cells: usize,
    ) -> Result<QueryResult<CollectedCustodianCells>> {
        const MAX_MERGE_SUDTS: usize = 5;
        const MIN_MERGE_CELLS: usize = 5;
        log::debug!(
            "sudt merge MIN_MERGE_CELLS {} MAX_MERGE_SUDTS {}",
            MIN_MERGE_CELLS,
            MAX_MERGE_SUDTS
        );

        let mut remain = max_cells.saturating_sub(collected.cells_info.len());
        if remain < MIN_MERGE_CELLS {
            log::debug!("sudt merge break remain < `MIN_MERGE_CELLS`");
            return Ok(QueryResult::NotEnough(collected));
        }

        let parse_sudt_amount = |info: &CellInfo| -> Result<u128> {
            if info.output.type_().is_none() {
                return Err(anyhow!("no a sudt cell"));
            }

            gw_types::packed::Uint128::from_slice(&info.data)
                .map(|a| a.unpack())
                .map_err(|e| anyhow!("invalid sudt amount {}", e))
        };

        let merge = |cells_info: Vec<CellInfo>,
                     collected_set: &mut HashSet<_>,
                     collected: &mut CollectedCustodianCells| {
            for info in cells_info {
                let sudt_amount = match parse_sudt_amount(&info) {
                    Ok(sudt_amount) => sudt_amount,
                    Err(_) => {
                        log::error!("unexpected invalid sudt amount error !!!!"); // Should already checked
                        continue;
                    }
                };
                let sudt_type_script = match info.output.type_().to_opt() {
                    Some(script) => script,
                    None => {
                        log::error!("unexpected none sudt type script !!!!"); // Should already checked
                        continue;
                    }
                };

                collected_set.insert(info.out_point.clone());

                let (collected_amount, _) = {
                    let sudt = collected.sudt.entry(sudt_type_script.hash());
                    sudt.or_insert((0, sudt_type_script))
                };
                *collected_amount = collected_amount.saturating_add(sudt_amount);

                collected.capacity = collected
                    .capacity
                    .saturating_add(info.output.capacity().unpack() as u128);
                collected.cells_info.push(info);
            }
        };

        let sudt_type_scripts = self
            .query_random_sudt_type_script(last_finalized_block_number, MAX_MERGE_SUDTS)
            .await?;
        log::info!("merge {} random sudt type scripts", sudt_type_scripts.len());
        let mut collected_set: HashSet<_> = {
            let cells = collected.cells_info.iter();
            cells.map(|i| i.out_point.clone()).collect()
        };
        for sudt_type_script in sudt_type_scripts {
            let query_result = self
                .query_mergeable_sudt_custodians_cells_by_sudt_type_script(
                    &sudt_type_script,
                    last_finalized_block_number,
                    remain,
                    &collected_set,
                )
                .await?;

            match query_result {
                QueryResult::Full(cells_info) => {
                    log::info!(
                        "merge (full) sudt custodians {} {}",
                        ckb_types::H256(sudt_type_script.hash()),
                        cells_info.len()
                    );
                    merge(cells_info, &mut collected_set, &mut collected)
                }
                QueryResult::NotEnough(cells_info) if cells_info.len() > 1 => {
                    log::info!(
                        "merge (not enough) sudt custodians {} {}",
                        ckb_types::H256(sudt_type_script.hash()),
                        cells_info.len()
                    );
                    merge(cells_info, &mut collected_set, &mut collected)
                }
                _ => continue,
            }

            remain = max_cells.saturating_sub(collected.cells_info.len());
            if remain < MIN_MERGE_CELLS {
                log::debug!(
                    "break `MIN_MERGE_CELLS` after sudt {} merge",
                    ckb_types::H256(sudt_type_script.hash())
                );
                break;
            }
        }

        if collected.cells_info.len() < max_cells {
            Ok(QueryResult::NotEnough(collected))
        } else {
            Ok(QueryResult::Full(collected))
        }
    }

    #[instrument(
        skip_all,
        fields(withdrawals_amount, last_finalized_block_number, max_cells)
    )]
    pub async fn query_finalized_custodian_cells(
        &self,
        withdrawals_amount: &WithdrawalsAmount,
        custodian_change_capacity: u128,
        last_finalized_block_number: u64,
        min_capacity: Option<u64>,
        max_cells: usize,
    ) -> Result<QueryResult<CollectedCustodianCells>> {
        Self::query_finalized_custodian_cells_from_indexer(
            &self.indexer,
            &self.rollup_context,
            withdrawals_amount,
            custodian_change_capacity,
            last_finalized_block_number,
            min_capacity,
            max_cells,
        )
        .await
    }

    // TODO: refactor into custodian mod
    #[instrument(skip_all)]
    async fn query_finalized_custodian_cells_from_indexer(
        indexed_cells: &impl IndexedCells,
        rollup_context: &RollupContext,
        withdrawals_amount: &WithdrawalsAmount,
        custodian_change_capacity: u128,
        last_finalized_block_number: u64,
        min_capacity: Option<u64>,
        max_cells: usize,
    ) -> Result<QueryResult<CollectedCustodianCells>> {
        const MAX_CELLS: usize = 50;

        let mut query_indexer_times = 0;
        let mut query_indexer_cells = 0;
        let now = Instant::now();

        let parse_sudt_amount = |cell: &Cell| -> Result<u128> {
            if cell.output.type_.is_none() {
                return Err(anyhow!("no a sudt cell"));
            }

            gw_types::packed::Uint128::from_slice(cell.output_data.as_bytes())
                .map(|a| a.unpack())
                .map_err(|e| anyhow!("invalid sudt amount {}", e))
        };

        let custodian_lock = Script::new_builder()
            .code_hash(rollup_context.rollup_config.custodian_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(rollup_context.rollup_script_hash.as_slice().pack())
            .build();
        let filter = min_capacity.map(|min_capacity| SearchKeyFilter {
            script: None,
            block_range: None,
            output_data_len_range: None,
            output_capacity_range: Some([min_capacity.into(), u64::MAX.into()]), // [inclusive, exclusive]
        });
        let search_key = SearchKey {
            script: ckb_types::packed::Script::new_unchecked(custodian_lock.as_bytes()).into(),
            script_type: ScriptType::Lock,
            filter,
        };
        // order by ASC so we can search more cells
        let order = Order::Asc;
        let limit = Uint32::from(DEFAULT_QUERY_LIMIT as u32);

        let mut collected = CollectedCustodianCells::default();
        let mut collected_fullfilled_sudt = HashSet::new();
        let mut cursor = None;

        // withdrawal ckb + change custodian capacity
        let required_capacity = {
            let withdrawal_capacity = withdrawals_amount.capacity;
            withdrawal_capacity.saturating_add(custodian_change_capacity)
        };

        while collected.capacity < required_capacity
            || collected_fullfilled_sudt.len() < withdrawals_amount.sudt.len()
        {
            let cells = indexed_cells
                .get_cells(&search_key, &order, &limit, cursor)
                .await?;

            if cells.last_cursor.is_empty() {
                return Ok(QueryResult::NotEnough(collected));
            }
            cursor = Some(cells.last_cursor);

            query_indexer_times += 1;
            query_indexer_cells += cells.objects.len();

            for cell in cells.objects.into_iter() {
                if collected.cells_info.len() >= max_cells {
                    return Ok(QueryResult::NotEnough(collected));
                }

                // Skip ckb custodians if capacity is fullfill
                if collected.capacity >= required_capacity
                    && !withdrawals_amount.sudt.is_empty()
                    && cell.output.type_.is_none()
                {
                    continue;
                }

                let args = cell.output.lock.args.clone().into_bytes();
                let custodian_lock_args = match CustodianLockArgsReader::verify(&args[32..], false)
                {
                    Ok(()) => CustodianLockArgs::new_unchecked(args.slice(32..)),
                    Err(_) => continue,
                };

                if custodian_lock_args.deposit_block_number().unpack() > last_finalized_block_number
                {
                    continue;
                }

                // Collect sudt
                if let Some(json_script) = cell.output.type_.clone() {
                    let sudt_type_script = {
                        let script = ckb_types::packed::Script::from(json_script);
                        Script::new_unchecked(script.as_bytes())
                    };

                    // Invalid custodian type script
                    let l1_sudt_script_type_hash =
                        rollup_context.rollup_config.l1_sudt_script_type_hash();
                    if sudt_type_script.code_hash() != l1_sudt_script_type_hash
                        || sudt_type_script.hash_type() != ScriptHashType::Type.into()
                    {
                        continue;
                    }

                    let sudt_type_hash = sudt_type_script.hash();
                    if sudt_type_hash != CKB_SUDT_SCRIPT_ARGS {
                        // Already collected enough sudt amount
                        if collected_fullfilled_sudt.contains(&sudt_type_hash) {
                            continue;
                        }

                        // Not target withdrawal sudt
                        let withdrawal_amount = match withdrawals_amount.sudt.get(&sudt_type_hash) {
                            Some(amount) => amount,
                            None => continue,
                        };

                        let sudt_amount = match parse_sudt_amount(&cell) {
                            Ok(amount) => amount,
                            Err(_) => {
                                log::error!("invalid sudt amount, out_point: {:?}", cell.out_point);
                                continue;
                            }
                        };

                        let (collected_amount, type_script) = {
                            let sudt = collected.sudt.entry(sudt_type_hash);
                            sudt.or_insert((0, Script::default()))
                        };
                        *collected_amount = collected_amount.saturating_add(sudt_amount);
                        *type_script = sudt_type_script;

                        if *collected_amount >= *withdrawal_amount {
                            collected_fullfilled_sudt.insert(sudt_type_hash);
                        }
                    }
                }

                // Collect capacity
                let out_point = {
                    let out_point: ckb_types::packed::OutPoint = cell.out_point.into();
                    OutPoint::new_unchecked(out_point.as_bytes())
                };

                let output = {
                    let output: ckb_types::packed::CellOutput = cell.output.into();
                    CellOutput::new_unchecked(output.as_bytes())
                };

                collected.capacity = collected
                    .capacity
                    .saturating_add(output.capacity().unpack() as u128);

                let info = CellInfo {
                    out_point,
                    output,
                    data: cell.output_data.into_bytes(),
                };

                collected.cells_info.push(info);

                if collected.cells_info.len() >= MAX_CELLS {
                    if collected.capacity >= required_capacity {
                        break;
                    } else {
                        log::debug!("[query finalized custodian cells] query indexer times: {} query indexer cells: {} duration: {}ms", query_indexer_times, query_indexer_cells, now.elapsed().as_millis());
                        return Ok(QueryResult::NotEnough(collected));
                    }
                }
            }
        }

        log::debug!("[query finalized custodian cells] query indexer times: {} query indexer cells: {} duration: {}ms", query_indexer_times, query_indexer_cells, now.elapsed().as_millis());
        Ok(QueryResult::Full(collected))
    }

    #[instrument(skip_all)]
    pub async fn query_verified_custodian_type_script(
        &self,
        sudt_script_hash: &[u8; 32],
    ) -> Result<Option<Script>> {
        let rollup_context = &self.rollup_context;

        let custodian_lock = Script::new_builder()
            .code_hash(rollup_context.rollup_config.custodian_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(rollup_context.rollup_script_hash.as_slice().pack())
            .build();

        let l1_sudt_type = Script::new_builder()
            .code_hash(rollup_context.rollup_config.l1_sudt_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .build();

        let search_key = SearchKey {
            script: ckb_types::packed::Script::new_unchecked(custodian_lock.as_bytes()).into(),
            script_type: ScriptType::Lock,
            filter: Some(SearchKeyFilter {
                script: Some(
                    ckb_types::packed::Script::new_unchecked(l1_sudt_type.as_bytes()).into(),
                ),
                output_data_len_range: None,
                output_capacity_range: None,
                block_range: None,
            }),
        };
        let order = Order::Desc;
        let limit = Uint32::from(DEFAULT_QUERY_LIMIT as u32);

        let mut cursor = None;
        loop {
            let cells: Pagination<Cell> = self
                .indexer
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
                return Ok(None);
            }
            cursor = Some(cells.last_cursor);

            for cell in cells.objects.into_iter() {
                let args = cell.output.lock.args.clone().into_bytes();
                if CustodianLockArgsReader::verify(&args[32..], false).is_err() {
                    continue;
                }

                let sudt_type_script = match cell.output.type_.clone() {
                    Some(json_script) => {
                        let script = ckb_types::packed::Script::from(json_script);
                        Script::new_unchecked(script.as_bytes())
                    }
                    None => continue,
                };

                if sudt_script_hash == &sudt_type_script.hash() {
                    return Ok(Some(sudt_type_script));
                }
            }
        }
    }

    #[instrument(skip_all)]
    pub async fn query_withdrawal_cells_by_block_hashes(
        &self,
        block_hashes: &HashSet<H256>,
    ) -> Result<(Vec<CellInfo>, HashSet<H256>)> {
        let rollup_context = &self.rollup_context;

        let withdrawal_lock = Script::new_builder()
            .code_hash(rollup_context.rollup_config.withdrawal_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(rollup_context.rollup_script_hash.as_slice().pack())
            .build();

        let search_key = SearchKey {
            script: ckb_types::packed::Script::new_unchecked(withdrawal_lock.as_bytes()).into(),
            script_type: ScriptType::Lock,
            filter: None,
        };
        let order = Order::Desc;
        let limit = Uint32::from(DEFAULT_QUERY_LIMIT as u32);

        let mut collected = vec![];
        let mut collected_block_hashes = HashSet::new();
        let mut cursor = None;

        while collected.is_empty() {
            let cells: Pagination<Cell> = self
                .indexer
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
                return Ok((collected, collected_block_hashes));
            }
            cursor = Some(cells.last_cursor);

            for cell in cells.objects.into_iter() {
                let args = cell.output.lock.args.clone().into_bytes();
                let withdrawal_lock_args =
                    match WithdrawalLockArgsReader::verify(&args[32..], false) {
                        Ok(()) => WithdrawalLockArgs::new_unchecked(args.slice(32..)),
                        Err(_) => continue,
                    };

                let withdrawal_block_hash: H256 =
                    withdrawal_lock_args.withdrawal_block_hash().unpack();
                if !block_hashes.contains(&withdrawal_block_hash) {
                    continue;
                }

                collected.push(to_cell_info(cell));
                collected_block_hashes.insert(withdrawal_block_hash);
            }
        }

        Ok((collected, collected_block_hashes))
    }

    #[instrument(skip_all)]
    pub async fn query_verifier_cell(
        &self,
        allowed_script_type_hash: [u8; 32],
        owner_lock_hash: [u8; 32],
    ) -> Result<Option<CellInfo>> {
        let lock = Script::new_builder()
            .code_hash(allowed_script_type_hash.pack())
            .hash_type(ScriptHashType::Type.into())
            .args(self.rollup_context.rollup_script_hash.as_slice().pack())
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

        let mut verifier_cell = None;
        let mut cursor = None;

        while verifier_cell.is_none() {
            let cells: Pagination<Cell> = self
                .indexer
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
                log::debug!(
                    "no verifier cell for script type hash {:?}",
                    allowed_script_type_hash
                );
                return Ok(None);
            }
            cursor = Some(cells.last_cursor);

            verifier_cell = cells.objects.into_iter().find_map(|cell| {
                if cell.output_data.len() >= 32
                    && cell.output_data.as_bytes()[0..32] == owner_lock_hash
                {
                    Some(to_cell_info(cell))
                } else {
                    None
                }
            });
        }

        Ok(verifier_cell)
    }

    #[instrument(skip_all)]
    pub async fn query_finalized_owner_lock_withdrawal_cells(
        &self,
        last_finalized_block_number: u64,
        exclusions: &HashSet<OutPoint>,
        max_cells: usize,
    ) -> Result<Vec<CellInfo>> {
        let rollup_context = &self.rollup_context;

        let withdrawal_lock = Script::new_builder()
            .code_hash(rollup_context.rollup_config.withdrawal_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(rollup_context.rollup_script_hash.as_slice().pack())
            .build();

        let search_key = SearchKey {
            script: ckb_types::packed::Script::new_unchecked(withdrawal_lock.as_bytes()).into(),
            script_type: ScriptType::Lock,
            filter: None,
        };
        let order = Order::Asc;
        let limit = Uint32::from(DEFAULT_QUERY_LIMIT as u32);

        let mut collected = vec![];
        let mut cursor = None;

        while collected.is_empty() {
            let cells: Pagination<Cell> = self
                .indexer
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

            for cell in cells.objects.into_iter() {
                let info = to_cell_info(cell);
                if exclusions.contains(&info.out_point) {
                    log::debug!("[finalized withdrawal] skip, in exclusions");
                    continue;
                }

                if let Err(err) = crate::withdrawal::verify_unlockable_to_owner(
                    &info,
                    last_finalized_block_number,
                    &rollup_context.rollup_config.l1_sudt_script_type_hash(),
                ) {
                    log::debug!("[finalized withdrawal] skip, verify failed {}", err);
                    continue;
                }

                collected.push(info);
                if collected.len() >= max_cells {
                    break;
                }
            }

            if cells.last_cursor.is_empty() {
                return Ok(collected);
            }
            cursor = Some(cells.last_cursor);
        }

        Ok(collected)
    }

    #[instrument(skip_all, fields(block_hash = %block_hash.pack()))]
    pub async fn get_header(
        &self,
        block_hash: H256,
    ) -> Result<Option<ckb_jsonrpc_types::HeaderView>> {
        let block: Option<ckb_jsonrpc_types::HeaderView> = self
            .ckb
            .request(
                "get_header",
                Some(ClientParams::Array(vec![json!(to_jsonh256(block_hash))])),
            )
            .await?;

        Ok(block)
    }

    #[instrument(skip_all, fields(number = %number))]
    pub async fn get_header_by_number(
        &self,
        number: u64,
    ) -> Result<Option<ckb_jsonrpc_types::HeaderView>> {
        let block_number = BlockNumber::from(number);
        let header: Option<ckb_jsonrpc_types::HeaderView> = self
            .ckb
            .request(
                "get_header_by_number",
                Some(ClientParams::Array(vec![json!(block_number)])),
            )
            .await?;

        Ok(header)
    }

    #[instrument(skip_all, fields(tx_hash = %tx.hash().pack()))]
    pub async fn send_transaction(&self, tx: &Transaction) -> Result<H256> {
        let tx: ckb_jsonrpc_types::Transaction = {
            let tx = ckb_types::packed::Transaction::new_unchecked(tx.as_bytes());
            tx.into()
        };
        let tx_hash: ckb_types::H256 = self
            .ckb
            .request(
                "send_transaction",
                Some(ClientParams::Array(vec![json!(tx), json!("passthrough")])),
            )
            .await?;
        Ok(to_h256(tx_hash))
    }

    #[instrument(skip_all)]
    pub async fn get_ckb_version(&self) -> Result<String> {
        let node: ckb_jsonrpc_types::LocalNode = self.ckb.request("local_node_info", None).await?;
        Ok(node.version)
    }

    #[instrument(skip_all, fields(tx_hash = %tx.hash().pack()))]
    pub async fn dry_run_transaction(&self, tx: &Transaction) -> Result<u64> {
        let tx: ckb_jsonrpc_types::Transaction = {
            let tx = ckb_types::packed::Transaction::new_unchecked(tx.as_bytes());
            tx.into()
        };
        let dry_run_result: ckb_jsonrpc_types::DryRunResult = self
            .ckb
            .request(
                "dry_run_transaction",
                Some(ClientParams::Array(vec![json!(tx)])),
            )
            .await?;
        Ok(dry_run_result.cycles.into())
    }

    #[instrument(skip_all)]
    pub async fn get_current_epoch_number(&self) -> Result<u64> {
        let epoch_view: ckb_jsonrpc_types::EpochView =
            self.ckb.request("get_current_epoch", None).await?;
        let epoch_number: u64 = epoch_view.number.into();
        Ok(epoch_number)
    }

    #[instrument(skip_all)]
    pub async fn get_hardfork_switch(&self) -> Result<HardForkSwitch> {
        let consensus: Consensus = self.ckb.request("get_consensus", None).await?;
        let rfc_0028 = self.get_hardfork_feature_epoch_number(&consensus, "0028")?;
        let rfc_0029 = self.get_hardfork_feature_epoch_number(&consensus, "0029")?;
        let rfc_0030 = self.get_hardfork_feature_epoch_number(&consensus, "0030")?;
        let rfc_0031 = self.get_hardfork_feature_epoch_number(&consensus, "0031")?;
        let rfc_0032 = self.get_hardfork_feature_epoch_number(&consensus, "0032")?;
        let rfc_0036 = self.get_hardfork_feature_epoch_number(&consensus, "0036")?;
        let rfc_0038 = self.get_hardfork_feature_epoch_number(&consensus, "0038")?;
        let hardfork_switch = HardForkSwitch::new_without_any_enabled()
            .as_builder()
            .rfc_0028(rfc_0028)
            .rfc_0029(rfc_0029)
            .rfc_0030(rfc_0030)
            .rfc_0031(rfc_0031)
            .rfc_0032(rfc_0032)
            .rfc_0036(rfc_0036)
            .rfc_0038(rfc_0038)
            .build()
            .map_err(|err| anyhow!(err))?;

        Ok(hardfork_switch)
    }

    #[instrument(skip_all)]
    fn get_hardfork_feature_epoch_number(&self, consensus: &Consensus, rfc: &str) -> Result<u64> {
        let rfc_info = consensus
            .hardfork_features
            .iter()
            .find(|f| f.rfc == rfc)
            .ok_or_else(|| anyhow!("rfc {} hardfork feature not found!", rfc))?;

        // if epoch_number is null, which means the fork will never going to happen
        let epoch_number: u64 = rfc_info.epoch_number.map(Into::into).unwrap_or(u64::MAX);
        Ok(epoch_number)
    }

    #[instrument(skip(self))]
    async fn query_random_sudt_type_script(
        &self,
        last_finalized_block_number: u64,
        max: usize,
    ) -> Result<HashSet<Script>> {
        let rollup_context = &self.rollup_context;

        let custodian_lock = Script::new_builder()
            .code_hash(rollup_context.rollup_config.custodian_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(rollup_context.rollup_script_hash.as_slice().pack())
            .build();
        let l1_sudt_type = Script::new_builder()
            .code_hash(rollup_context.rollup_config.l1_sudt_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .build();
        let filter = Some(SearchKeyFilter {
            script: Some(ckb_types::packed::Script::new_unchecked(l1_sudt_type.as_bytes()).into()),
            block_range: None,
            output_data_len_range: Some([16.into(), u64::MAX.into()]),
            output_capacity_range: None,
        });
        let search_key = SearchKey {
            script: ckb_types::packed::Script::new_unchecked(custodian_lock.as_bytes()).into(),
            script_type: ScriptType::Lock,
            filter,
        };
        let order = Order::Desc;
        let limit = Uint32::from(DEFAULT_QUERY_LIMIT as u32);

        let mut sudt_type_script_set = HashSet::new();
        let mut cursor = None;
        while sudt_type_script_set.len() < max {
            let cells: Pagination<Cell> = self
                .indexer
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

            for cell in cells.objects.into_iter() {
                if sudt_type_script_set.len() >= max {
                    return Ok(sudt_type_script_set);
                }

                let info = to_cell_info(cell);
                let sudt_type_script = match info.output.type_().to_opt() {
                    Some(sudt_type_script) => sudt_type_script,
                    None => continue,
                };
                if sudt_type_script_set.contains(&sudt_type_script) {
                    continue;
                }
                if random::<u32>() % 2 != 0 {
                    continue;
                }

                let args: Bytes = info.output.lock().args().unpack();
                let custodian_lock_args = match CustodianLockArgsReader::verify(&args[32..], false)
                {
                    Ok(()) => CustodianLockArgs::new_unchecked(args.slice(32..)),
                    Err(_) => continue,
                };
                if custodian_lock_args.deposit_block_number().unpack() > last_finalized_block_number
                {
                    continue;
                }

                // Double check invalid custodian type script
                let l1_sudt_script_type_hash =
                    rollup_context.rollup_config.l1_sudt_script_type_hash();
                if sudt_type_script.code_hash() != l1_sudt_script_type_hash
                    || sudt_type_script.hash_type() != ScriptHashType::Type.into()
                {
                    continue;
                }

                sudt_type_script_set.insert(sudt_type_script);
            }

            if cells.last_cursor.is_empty() {
                break;
            }
            cursor = Some(cells.last_cursor);
        }

        Ok(sudt_type_script_set)
    }

    #[instrument(skip_all, fields(last_finalized_block_number = last_finalized_block_number, max_cells = max_cells))]
    async fn query_mergeable_sudt_custodians_cells_by_sudt_type_script(
        &self,
        sudt_type_script: &Script,
        last_finalized_block_number: u64,
        max_cells: usize,
        exclusions: &HashSet<OutPoint>,
    ) -> Result<QueryResult<Vec<CellInfo>>> {
        let rollup_context = &self.rollup_context;

        let parse_sudt_amount = |info: &CellInfo| -> Result<u128> {
            if info.output.type_().is_none() {
                return Err(anyhow!("no a sudt cell"));
            }

            gw_types::packed::Uint128::from_slice(&info.data)
                .map(|a| a.unpack())
                .map_err(|e| anyhow!("invalid sudt amount {}", e))
        };

        let custodian_lock = Script::new_builder()
            .code_hash(rollup_context.rollup_config.custodian_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(rollup_context.rollup_script_hash.as_slice().pack())
            .build();
        let filter = Some(SearchKeyFilter {
            script: Some(
                ckb_types::packed::Script::new_unchecked(sudt_type_script.as_bytes()).into(),
            ),
            block_range: None,
            output_data_len_range: Some([16.into(), u64::MAX.into()]),
            output_capacity_range: None,
        });
        let search_key = SearchKey {
            script: ckb_types::packed::Script::new_unchecked(custodian_lock.as_bytes()).into(),
            script_type: ScriptType::Lock,
            filter,
        };
        let order = Order::Desc;
        let limit = Uint32::from(DEFAULT_QUERY_LIMIT as u32);

        let mut collected = Vec::new();
        let mut collected_set = exclusions.clone();
        let mut cursor = None;
        while collected.len() < max_cells {
            let cells: Pagination<Cell> = self
                .indexer
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

            for cell in cells.objects.into_iter() {
                if collected.len() >= max_cells {
                    return Ok(QueryResult::Full(collected));
                }

                let info = to_cell_info(cell);
                if collected_set.contains(&info.out_point) {
                    continue;
                }

                let args: Bytes = info.output.lock().args().unpack();
                let custodian_lock_args = match CustodianLockArgsReader::verify(&args[32..], false)
                {
                    Ok(()) => CustodianLockArgs::new_unchecked(args.slice(32..)),
                    Err(_) => continue,
                };

                if custodian_lock_args.deposit_block_number().unpack() > last_finalized_block_number
                {
                    continue;
                }

                match info.output.type_().to_opt() {
                    Some(type_script) if type_script.hash() != sudt_type_script.hash() => continue,
                    None => continue,
                    _ => (),
                };

                if parse_sudt_amount(&info).is_err() {
                    log::error!("invalid sudt amount, out_point: {:?}", info.out_point);
                    continue;
                }

                collected_set.insert(info.out_point.clone());
                collected.push(info);
            }

            if cells.last_cursor.is_empty() {
                break;
            }
            cursor = Some(cells.last_cursor);
        }

        if collected.len() < max_cells {
            Ok(QueryResult::NotEnough(collected))
        } else {
            Ok(QueryResult::Full(collected))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RPCClient;

    use std::collections::HashMap;

    use anyhow::Result;
    use async_trait::async_trait;
    use gw_jsonrpc_types::ckb_jsonrpc_types::{JsonBytes, Uint32};
    use gw_types::bytes::Bytes;
    use gw_types::core::ScriptHashType;
    use gw_types::offchain::{RollupContext, WithdrawalsAmount};
    use gw_types::packed::{CellOutput, CustodianLockArgs, RollupConfig, Script, Uint128};
    use gw_types::prelude::{Builder, Entity, Pack};

    use crate::indexer_types::{Cell, Order, Pagination, SearchKey};
    use crate::rpc_client::QueryResult;
    use crate::traits::IndexedCells;

    const CKB: u64 = 100_000_000;

    struct DummyIndexedCells {
        cells: Vec<Cell>,
    }

    #[async_trait]
    impl IndexedCells for DummyIndexedCells {
        async fn get_cells(
            &self,
            _search_key: &SearchKey,
            _order: &Order,
            _limit: &Uint32,
            _cursor: Option<JsonBytes>,
        ) -> Result<Pagination<Cell>> {
            Ok(Pagination {
                objects: serde_json::from_str(&serde_json::to_string(&self.cells).unwrap())
                    .unwrap(),
                last_cursor: JsonBytes::from_bytes(Bytes::from_static(b"1")),
            })
        }
    }

    #[tokio::test]
    async fn test_query_finalized_custodians() {
        let rollup_context = RollupContext {
            rollup_script_hash: [1u8; 32].into(),
            rollup_config: RollupConfig::new_builder()
                .custodian_script_type_hash([2u8; 32].pack())
                .l1_sudt_script_type_hash([3u8; 32].pack())
                .build(),
        };

        let sudt_script = Script::new_builder()
            .code_hash([3u8; 32].pack())
            .hash_type(ScriptHashType::Type.into())
            .args(Bytes::from_static(b"33").pack())
            .build();

        let withdrawals_amount = WithdrawalsAmount {
            capacity: (1000 * CKB) as u128,
            sudt: HashMap::from([(sudt_script.hash(), 500u128); 1]),
        };

        const FINALIZED_BLOCK_NUMBER: u64 = 100;
        let ten_ckb_cells = generate_finalized_ckb_custodian_cells(
            10,
            &rollup_context,
            FINALIZED_BLOCK_NUMBER,
            1000 * CKB,
        );
        let one_sudt_cell = generate_finalized_sudt_custodian_cells(
            1,
            &rollup_context,
            FINALIZED_BLOCK_NUMBER,
            1000 * CKB,
            sudt_script.clone(),
            1000u128.pack(),
        );

        let max_five_cells = 5;
        let change_capacity = 0;
        let dummy_indexed_cells = DummyIndexedCells {
            cells: vec![ten_ckb_cells, one_sudt_cell]
                .into_iter()
                .flatten()
                .collect(),
        };

        let query_finalized_custodians = RPCClient::query_finalized_custodian_cells_from_indexer(
            &dummy_indexed_cells,
            &rollup_context,
            &withdrawals_amount,
            change_capacity,
            FINALIZED_BLOCK_NUMBER,
            None,
            max_five_cells,
        );

        assert!(matches!(
            query_finalized_custodians.await.unwrap(),
            QueryResult::Full(_)
        ));
    }

    fn generate_finalized_ckb_custodian_cells(
        cell_num: usize,
        rollup_context: &RollupContext,
        last_finalized_block_number: u64,
        capacity: u64,
    ) -> Vec<Cell> {
        let args = {
            let custodian_lock_args = CustodianLockArgs::new_builder()
                .deposit_block_number(last_finalized_block_number.pack())
                .build();

            let mut args = rollup_context.rollup_script_hash.as_slice().to_vec();
            args.extend_from_slice(custodian_lock_args.as_slice());

            Bytes::from(args)
        };
        let lock = Script::new_builder()
            .code_hash(rollup_context.rollup_config.custodian_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(args.pack())
            .build();
        let output = CellOutput::new_builder()
            .capacity(capacity.pack())
            .lock(lock)
            .build();

        (0..cell_num)
            .map(|_| Cell {
                output: ckb_types::packed::CellOutput::new_unchecked(output.as_bytes()).into(),
                output_data: JsonBytes::default(),
                out_point: ckb_types::packed::OutPoint::default().into(),
                block_number: 10u64.into(),
                tx_index: 0u32.into(),
            })
            .collect()
    }

    fn generate_finalized_sudt_custodian_cells(
        cell_num: usize,
        rollup_context: &RollupContext,
        last_finalized_block_number: u64,
        capacity: u64,
        sudt_script: Script,
        amount: Uint128,
    ) -> Vec<Cell> {
        use ckb_types::prelude::Pack;

        let ckb_cells = generate_finalized_ckb_custodian_cells(
            cell_num,
            rollup_context,
            last_finalized_block_number,
            capacity,
        );

        let sudt_script = ckb_types::packed::Script::new_unchecked(sudt_script.as_bytes());
        let convert_to_sudt = |cell: Cell| {
            let output: ckb_types::packed::CellOutput = cell.output.into();
            let sudt_output = output
                .as_builder()
                .type_(Some(sudt_script.clone()).pack())
                .build();

            Cell {
                output: sudt_output.into(),
                output_data: JsonBytes::from_bytes(amount.as_bytes()),
                ..cell
            }
        };
        ckb_cells.into_iter().map(convert_to_sudt).collect()
    }
}
