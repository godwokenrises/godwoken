#![allow(clippy::mutable_key_type)]

use std::{collections::HashSet, time::Duration};

use anyhow::{anyhow, Result};
use gw_jsonrpc_types::ckb_jsonrpc_types::{self, BlockNumber, OutputsValidator, Uint32};
use gw_types::{
    bytes::Bytes,
    core::{ScriptHashType, Timepoint},
    h256::H256,
    offchain::{CellInfo, CellStatus, CellWithStatus, CompatibleFinalizedTimepoint, DepositInfo},
    packed::{
        Block, CellOutput, CustodianLockArgs, CustodianLockArgsReader, DepositLockArgs,
        DepositLockArgsReader, DepositRequest, NumberHash, OutPoint, RollupConfig, Script,
        StakeLockArgs, StakeLockArgsReader, Transaction, WithdrawalLockArgs,
        WithdrawalLockArgsReader,
    },
    prelude::{Entity, *},
};
use rand::prelude::*;
use tracing::instrument;

use crate::{
    ckb_client::CkbClient,
    indexer_client::CkbIndexerClient,
    indexer_types::{Cell, Order, ScriptType, SearchKey, SearchKeyFilter},
    utils::DEFAULT_QUERY_LIMIT,
};

fn to_cell_info(cell: Cell) -> CellInfo {
    let out_point = cell.out_point.into();
    let output = cell.output.into();
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
    pub indexer: CkbIndexerClient,
    pub ckb: CkbClient,
    pub rollup_type_script: ckb_types::packed::Script,
    pub rollup_config: RollupConfig,
}

impl RPCClient {
    pub fn new(
        rollup_type_script: ckb_types::packed::Script,
        rollup_config: RollupConfig,
        ckb: CkbClient,
        indexer: CkbIndexerClient,
    ) -> Self {
        Self {
            indexer,
            ckb,
            rollup_type_script,
            rollup_config,
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

        let mut cells = self
            .indexer
            .get_cells(&search_key, &order, limit, &None)
            .await?;
        if let Some(cell) = cells.objects.pop() {
            return Ok(Some(cell.info()));
        }
        Ok(None)
    }

    /// this function return a cell that do not has data & _type fields
    #[instrument(skip_all)]
    pub async fn query_owner_cell(
        &self,
        lock: Script,
        filter_inputs: Option<HashSet<OutPoint>>,
    ) -> Result<Option<CellInfo>> {
        let search_key = SearchKey {
            script: lock.into(),
            script_type: ScriptType::Lock,
            filter: None,
        };
        let order = Order::Desc;
        let limit = Uint32::from(DEFAULT_QUERY_LIMIT as u32);

        let mut cell = None;
        let mut cursor = None;
        while cell.is_none() {
            let cells = self
                .indexer
                .get_cells(&search_key, &order, limit, &cursor)
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
                let out_point = cell.out_point.clone().into();
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
        let cell_with_status = self
            .ckb
            .get_live_cell(out_point.clone().into(), true)
            .await?;
        let cell_info = cell_with_status.cell.map(|cell| {
            let output: ckb_types::packed::CellOutput = cell.output.into();
            let data = cell
                .data
                .map(|cell_data| cell_data.content.into_bytes())
                .unwrap_or_else(Bytes::new);
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
            .get_packed_transaction(out_point.tx_hash().unpack())
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

    pub async fn get_tip(&self) -> Result<NumberHash> {
        self.indexer.get_indexer_tip1().await.map(Into::into)
    }

    pub async fn get_block_median_time(&self, block_hash: H256) -> Result<Option<Duration>> {
        let opt_median_time = self.ckb.get_block_median_time(block_hash.into()).await?;
        Ok(opt_median_time.map(|t| Duration::from_millis(t.into())))
    }

    pub async fn get_block_by_number(&self, number: u64) -> Result<Option<Block>> {
        let block_opt = self.ckb.get_block_by_number(number.into()).await?;
        Ok(block_opt.map(|b| {
            let block: ckb_types::core::BlockView = b.into();
            block.data()
        }))
    }

    /// return all lived deposit requests
    /// NOTICE the returned cells may contains invalid cells.
    #[instrument(skip(self, dead_cells))]
    pub async fn query_deposit_cells(
        &self,
        count: usize,
        deposit_minimal_blocks: u64,
        min_ckb_deposit_capacity: u64,
        min_sudt_deposit_capacity: u64,
        dead_cells: &HashSet<OutPoint>,
    ) -> Result<Vec<DepositInfo>> {
        const BLOCKS_TO_SEARCH: u64 = 2000;

        let tip_number: u64 = self.get_tip().await?.number().unpack();
        let mut deposit_infos = Vec::new();

        let script = Script::new_builder()
            .code_hash(self.rollup_config.deposit_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(self.rollup_type_script.calc_script_hash().as_bytes().pack())
            .build();

        let from_block = tip_number.saturating_sub(BLOCKS_TO_SEARCH);
        let to_block = tip_number.saturating_sub(deposit_minimal_blocks);

        log::debug!(target: "collect-deposit-cells", "start searching deposit cells from_block {} to_block {} count {} min_ckb_deposit_capacity {} min_sudt_deposit_capacity {}",
             from_block, to_block, count, min_ckb_deposit_capacity, min_sudt_deposit_capacity);

        let search_key = SearchKey {
            script: script.into(),
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

            let cells = self
                .indexer
                .get_cells(&search_key, &order, limit, &cursor)
                .await?;

            log::debug!(target: "collect-deposit-cells", "query {} cells", cells.objects.len());

            if cells.last_cursor.is_empty() {
                break;
            }
            cursor = Some(cells.last_cursor);

            let cells = cells.objects.into_iter().map(|cell| cell.info());

            for cell in cells {
                // Ensure finalized ckb custodians are clearly mergeable
                if dead_cells.contains(&cell.out_point)
                    || cell.output.type_().is_none() && !cell.data.is_empty()
                {
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

                let cell_capacity: u64 = cell.output.capacity().unpack();
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

    #[instrument(skip_all)]
    pub async fn query_stake_cells_by_owner_lock_hashes(
        &self,
        owner_lock_hashes: impl Iterator<Item = [u8; 32]>,
    ) -> Result<Vec<CellInfo>> {
        let lock = Script::new_builder()
            .code_hash(self.rollup_config.stake_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(self.rollup_type_script.calc_script_hash().as_bytes().pack())
            .build();

        let search_key = SearchKey {
            script: lock.into(),
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
            let cells = self
                .indexer
                .get_cells(&search_key, &order, limit, &cursor)
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
        let custodian_lock = Script::new_builder()
            .code_hash(self.rollup_config.custodian_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(self.rollup_type_script.calc_script_hash().as_bytes().pack())
            .build();

        let search_key = SearchKey {
            script: custodian_lock.into(),
            script_type: ScriptType::Lock,
            filter: None,
        };
        let order = Order::Desc;
        let limit = Uint32::from(DEFAULT_QUERY_LIMIT as u32);

        let mut collected = vec![];
        let mut collected_block_hashes = HashSet::new();
        let mut cursor = None;

        while collected.is_empty() {
            let cells = self
                .indexer
                .get_cells(&search_key, &order, limit, &cursor)
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

    #[instrument(skip_all)]
    pub async fn query_verified_custodian_type_script(
        &self,
        sudt_script_hash: &[u8; 32],
    ) -> Result<Option<Script>> {
        let custodian_lock = Script::new_builder()
            .code_hash(self.rollup_config.custodian_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(self.rollup_type_script.calc_script_hash().as_bytes().pack())
            .build();

        let l1_sudt_type = Script::new_builder()
            .code_hash(self.rollup_config.l1_sudt_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .build();

        let search_key = SearchKey {
            script: custodian_lock.into(),
            script_type: ScriptType::Lock,
            filter: Some(SearchKeyFilter {
                script: Some(l1_sudt_type.into()),
                output_data_len_range: None,
                output_capacity_range: None,
                block_range: None,
            }),
        };
        let order = Order::Desc;
        let limit = Uint32::from(DEFAULT_QUERY_LIMIT as u32);

        let mut cursor = None;
        loop {
            let cells = self
                .indexer
                .get_cells(&search_key, &order, limit, &cursor)
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

                let sudt_type_script: Script = match cell.output.type_.clone() {
                    Some(json_script) => json_script.into(),
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
        let withdrawal_lock = Script::new_builder()
            .code_hash(self.rollup_config.withdrawal_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(self.rollup_type_script.calc_script_hash().as_bytes().pack())
            .build();

        let search_key = SearchKey {
            script: withdrawal_lock.into(),
            script_type: ScriptType::Lock,
            filter: None,
        };
        let order = Order::Desc;
        let limit = Uint32::from(DEFAULT_QUERY_LIMIT as u32);

        let mut collected = vec![];
        let mut collected_block_hashes = HashSet::new();
        let mut cursor = None;

        while collected.is_empty() {
            let cells = self
                .indexer
                .get_cells(&search_key, &order, limit, &cursor)
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
            .args(self.rollup_type_script.calc_script_hash().as_bytes().pack())
            .build();

        let search_key = SearchKey {
            script: lock.into(),
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
            let cells = self
                .indexer
                .get_cells(&search_key, &order, limit, &cursor)
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

    #[instrument(skip_all, err(Debug), fields(timepoint = ?compatible_finalized_timepoint))]
    pub async fn query_finalized_owner_lock_withdrawal_cells(
        &self,
        compatible_finalized_timepoint: &CompatibleFinalizedTimepoint,
        exclusions: &HashSet<OutPoint>,
        max_cells: usize,
    ) -> Result<Vec<CellInfo>> {
        let withdrawal_lock = Script::new_builder()
            .code_hash(self.rollup_config.withdrawal_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(self.rollup_type_script.calc_script_hash().as_bytes().pack())
            .build();

        let search_key = SearchKey::with_lock(withdrawal_lock);
        let order = Order::Asc;
        let limit = Uint32::from(DEFAULT_QUERY_LIMIT as u32);

        let mut collected = vec![];
        let mut cursor = None;

        while collected.is_empty() {
            let cells = self
                .indexer
                .get_cells(&search_key, &order, limit, &cursor)
                .await?;

            for cell in cells.objects.into_iter() {
                let info = to_cell_info(cell);
                if exclusions.contains(&info.out_point) {
                    log::debug!("[finalized withdrawal] skip, in exclusions");
                    continue;
                }

                if let Err(err) = crate::withdrawal::verify_unlockable_to_owner(
                    &info,
                    compatible_finalized_timepoint,
                    &self.rollup_config.l1_sudt_script_type_hash(),
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

    pub async fn get_header(
        &self,
        block_hash: H256,
    ) -> Result<Option<ckb_jsonrpc_types::HeaderView>> {
        self.ckb.get_header(block_hash.into()).await
    }

    pub async fn get_header_by_number(
        &self,
        number: u64,
    ) -> Result<Option<ckb_jsonrpc_types::HeaderView>> {
        self.ckb.get_header_by_number(number.into()).await
    }

    pub async fn send_transaction(&self, tx: &Transaction) -> Result<H256> {
        let tx = tx.clone().into();
        let tx_hash = self
            .ckb
            .send_transaction(tx, Some(OutputsValidator::Passthrough))
            .await?;
        Ok(tx_hash.into())
    }

    pub async fn get_ckb_version(&self) -> Result<String> {
        let node = self.ckb.local_node_info().await?;
        Ok(node.version)
    }

    pub async fn dry_run_transaction(&self, tx: &Transaction) -> Result<u64> {
        let tx = tx.clone().into();
        let cycles = self.ckb.estimate_cycles(tx).await?;
        Ok(cycles.cycles.into())
    }

    #[instrument(skip_all, err(Debug), fields(timepoint = ?compatible_finalized_timepoint))]
    pub async fn query_random_sudt_type_script(
        &self,
        compatible_finalized_timepoint: &CompatibleFinalizedTimepoint,
        max: usize,
    ) -> Result<HashSet<Script>> {
        let custodian_lock = Script::new_builder()
            .code_hash(self.rollup_config.custodian_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(self.rollup_type_script.calc_script_hash().as_bytes().pack())
            .build();
        let l1_sudt_type = Script::new_builder()
            .code_hash(self.rollup_config.l1_sudt_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .build();
        let filter = Some(SearchKeyFilter {
            script: Some(l1_sudt_type.into()),
            block_range: None,
            output_data_len_range: Some([16.into(), u64::MAX.into()]),
            output_capacity_range: None,
        });
        let search_key = SearchKey {
            script: custodian_lock.into(),
            script_type: ScriptType::Lock,
            filter,
        };
        let order = Order::Desc;
        let limit = Uint32::from(DEFAULT_QUERY_LIMIT as u32);

        let mut sudt_type_script_set = HashSet::new();
        let mut cursor = None;
        while sudt_type_script_set.len() < max {
            let cells = self
                .indexer
                .get_cells(&search_key, &order, limit, &cursor)
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
                if !compatible_finalized_timepoint.is_finalized(&Timepoint::from_full_value(
                    custodian_lock_args.deposit_finalized_timepoint().unpack(),
                )) {
                    continue;
                }

                // Double check invalid custodian type script
                let l1_sudt_script_type_hash = self.rollup_config.l1_sudt_script_type_hash();
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

    #[instrument(skip_all, err(Debug), fields(timepoint = ?compatible_finalized_timepoint, max_cells = max_cells))]
    pub async fn query_mergeable_sudt_custodians_cells_by_sudt_type_script(
        &self,
        sudt_type_script: &Script,
        compatible_finalized_timepoint: &CompatibleFinalizedTimepoint,
        max_cells: usize,
        exclusions: &HashSet<OutPoint>,
    ) -> Result<QueryResult<Vec<CellInfo>>> {
        let parse_sudt_amount = |info: &CellInfo| -> Result<u128> {
            if info.output.type_().is_none() {
                return Err(anyhow!("no a sudt cell"));
            }

            gw_types::packed::Uint128::from_slice(&info.data)
                .map(|a| a.unpack())
                .map_err(|e| anyhow!("invalid sudt amount {}", e))
        };

        let custodian_lock = Script::new_builder()
            .code_hash(self.rollup_config.custodian_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(self.rollup_type_script.calc_script_hash().as_bytes().pack())
            .build();
        let filter = Some(SearchKeyFilter {
            script: Some(sudt_type_script.clone().into()),
            block_range: None,
            output_data_len_range: Some([16.into(), u64::MAX.into()]),
            output_capacity_range: None,
        });
        let search_key = SearchKey {
            script: custodian_lock.into(),
            script_type: ScriptType::Lock,
            filter,
        };
        let order = Order::Desc;
        let limit = Uint32::from(DEFAULT_QUERY_LIMIT as u32);

        let mut collected = Vec::new();
        let mut collected_set = exclusions.clone();
        let mut cursor = None;
        while collected.len() < max_cells {
            let cells = self
                .indexer
                .get_cells(&search_key, &order, limit, &cursor)
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

                if !compatible_finalized_timepoint.is_finalized(&Timepoint::from_full_value(
                    custodian_lock_args.deposit_finalized_timepoint().unpack(),
                )) {
                    continue;
                }

                match info.output.type_().to_opt() {
                    Some(type_script) if type_script != *sudt_type_script => continue,
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
