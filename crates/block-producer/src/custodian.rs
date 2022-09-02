#![allow(clippy::mutable_key_type)]

use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, bail, Result};
use gw_rpc_client::{
    indexer_types::{Order, SearchKey, SearchKeyFilter},
    rpc_client::{QueryResult, RPCClient},
};
use gw_types::{
    core::ScriptHashType,
    offchain::{CellInfo, CollectedCustodianCells},
    packed::{CustodianLockArgsReader, Script},
    prelude::*,
};
use gw_utils::local_cells::{
    collect_local_and_indexer_cells, CollectLocalAndIndexerCursor, LocalCellsManager,
};

use gw_mem_pool::custodian::{
    build_finalized_custodian_lock, calc_ckb_custodian_min_capacity, generate_finalized_custodian,
    AvailableCustodians,
};
use gw_types::{
    bytes::Bytes,
    offchain::{InputCellInfo, RollupContext, WithdrawalsAmount},
    packed::{CellInput, CellOutput},
};
use tracing::instrument;

pub const MAX_CUSTODIANS: usize = 50;

#[instrument(skip_all, fields(last_finalized_block_number = last_finalized_block_number))]
pub async fn query_mergeable_custodians(
    local_cells_manager: &LocalCellsManager,
    rpc_client: &RPCClient,
    collected_custodians: CollectedCustodianCells,
    last_finalized_block_number: u64,
) -> Result<QueryResult<CollectedCustodianCells>> {
    if collected_custodians.cells_info.len() >= MAX_CUSTODIANS {
        return Ok(QueryResult::Full(collected_custodians));
    }

    let query_result = query_mergeable_ckb_custodians(
        local_cells_manager,
        rpc_client,
        collected_custodians,
        last_finalized_block_number,
        MAX_CUSTODIANS,
    )
    .await?;
    if matches!(query_result, QueryResult::Full(_)) {
        return Ok(query_result);
    }

    query_mergeable_sudt_custodians(
        rpc_client,
        query_result.expect_any(),
        last_finalized_block_number,
        local_cells_manager,
    )
    .await
}

pub struct AggregatedCustodians {
    pub inputs: Vec<InputCellInfo>,
    pub outputs: Vec<(CellOutput, Bytes)>,
}

pub fn aggregate_balance(
    rollup_context: &RollupContext,
    finalized_custodians: CollectedCustodianCells,
    withdrawals_amount: WithdrawalsAmount,
) -> Result<Option<AggregatedCustodians>> {
    // No enough custodians to merge
    if withdrawals_amount.is_zero() && finalized_custodians.cells_info.len() <= 1 {
        return Ok(None);
    }

    let available_custodians = AvailableCustodians {
        capacity: finalized_custodians.capacity,
        sudt: finalized_custodians.sudt,
    };

    let mut aggregator = Aggregator::new(rollup_context, available_custodians);
    aggregator.minus_withdrawals(withdrawals_amount)?;

    let custodian_inputs = finalized_custodians.cells_info.into_iter().map(|cell| {
        let input = CellInput::new_builder()
            .previous_output(cell.out_point.clone())
            .build();
        InputCellInfo { input, cell }
    });

    let aggregated = AggregatedCustodians {
        inputs: custodian_inputs.collect(),
        outputs: aggregator.finish(),
    };

    Ok(Some(aggregated))
}

#[instrument(skip_all, fields(last_finalized_block_number = last_finalized_block_number))]
async fn query_mergeable_sudt_custodians(
    rpc_client: &RPCClient,
    collected: CollectedCustodianCells,
    last_finalized_block_number: u64,
    local_cells_manager: &LocalCellsManager,
) -> Result<QueryResult<CollectedCustodianCells>> {
    if collected.cells_info.len() >= MAX_CUSTODIANS {
        return Ok(QueryResult::Full(collected));
    }

    query_mergeable_sudt_custodians_cells(
        local_cells_manager,
        rpc_client,
        collected,
        last_finalized_block_number,
        MAX_CUSTODIANS,
    )
    .await
}

async fn query_mergeable_ckb_custodians(
    local_cells_manager: &LocalCellsManager,
    rpc_client: &RPCClient,
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

    let rollup_context = &rpc_client.rollup_context;
    let custodian_lock = Script::new_builder()
        .code_hash(rollup_context.rollup_config.custodian_script_type_hash())
        .hash_type(ScriptHashType::Type.into())
        .args(rollup_context.rollup_script_hash.as_slice().pack())
        .build();
    let filter = Some(SearchKeyFilter {
        output_data_len_range: Some([0.into(), 1.into()]), // [inclusive, exclusive]
        ..Default::default()
    });
    let search_key = SearchKey::with_lock(custodian_lock).with_filter(filter);
    let order = Order::Desc;

    let mut collected_set: HashSet<_> = {
        let cells = collected.cells_info.iter();
        cells.map(|i| i.out_point.clone()).collect()
    };

    let mut cursor = CollectLocalAndIndexerCursor::Local;
    let mut collected_ckb_custodians = Vec::<CellInfo>::with_capacity(remain);
    while collected_ckb_custodians.len() < remain && !cursor.is_ended() {
        let cells = collect_local_and_indexer_cells(
            local_cells_manager,
            &rpc_client.indexer,
            &search_key,
            &order,
            None,
            &mut cursor,
        )
        .await?;

        for cell in cells {
            if collected.cells_info.len() >= max_cells {
                return Ok(QueryResult::Full(collected));
            }

            if collected_set.contains(&cell.out_point) {
                continue;
            }

            let args = cell.output.lock().args().raw_data();
            let custodian_lock_args_reader = match CustodianLockArgsReader::from_slice(&args[32..])
            {
                Ok(r) => r,
                Err(_) => continue,
            };
            if custodian_lock_args_reader.deposit_block_number().unpack()
                > last_finalized_block_number
            {
                continue;
            }

            collected_set.insert(cell.out_point.clone());
            collected_ckb_custodians.push(cell);
        }
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

// TODO: use local live.
pub async fn query_mergeable_sudt_custodians_cells(
    local_cells_manager: &LocalCellsManager,
    rpc_client: &RPCClient,
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
            bail!("no a sudt cell");
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

    let sudt_type_scripts = rpc_client
        .query_random_sudt_type_script(last_finalized_block_number, MAX_MERGE_SUDTS)
        .await?;
    log::info!("merge {} random sudt type scripts", sudt_type_scripts.len());
    let mut collected_set: HashSet<_> = {
        let mut local = local_cells_manager.dead_cells().clone();

        let cells = collected.cells_info.iter();
        local.extend(cells.map(|i| i.out_point.clone()));
        local
    };
    for sudt_type_script in sudt_type_scripts {
        let query_result = rpc_client
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

#[derive(Clone)]
struct CkbCustodian {
    capacity: u128,
    balance: u128,
    min_capacity: u64,
}

struct SudtCustodian {
    capacity: u64,
    balance: u128,
    script: Script,
}

struct Aggregator<'a> {
    rollup_context: &'a RollupContext,
    ckb_custodian: CkbCustodian,
    sudt_custodians: HashMap<[u8; 32], SudtCustodian>,
}

impl<'a> Aggregator<'a> {
    fn new(rollup_context: &'a RollupContext, available_custodians: AvailableCustodians) -> Self {
        let mut total_sudt_capacity = 0u128;
        let mut sudt_custodians = HashMap::new();

        for (sudt_type_hash, (balance, type_script)) in available_custodians.sudt.into_iter() {
            let (change, _data) =
                generate_finalized_custodian(rollup_context, balance, type_script.clone());

            let sudt_custodian = SudtCustodian {
                capacity: change.capacity().unpack(),
                balance,
                script: type_script,
            };

            total_sudt_capacity =
                total_sudt_capacity.saturating_add(sudt_custodian.capacity as u128);
            sudt_custodians.insert(sudt_type_hash, sudt_custodian);
        }

        let ckb_custodian_min_capacity = calc_ckb_custodian_min_capacity(rollup_context);
        let ckb_custodian_capacity = available_custodians
            .capacity
            .saturating_sub(total_sudt_capacity);
        let ckb_balance = ckb_custodian_capacity.saturating_sub(ckb_custodian_min_capacity as u128);

        let ckb_custodian = CkbCustodian {
            capacity: ckb_custodian_capacity,
            balance: ckb_balance,
            min_capacity: ckb_custodian_min_capacity,
        };

        Aggregator {
            rollup_context,
            ckb_custodian,
            sudt_custodians,
        }
    }

    fn minus_withdrawals(&mut self, withdrawals_amount: WithdrawalsAmount) -> Result<()> {
        let ckb_custodian = &mut self.ckb_custodian;

        for (sudt_type_hash, amount) in withdrawals_amount.sudt {
            let sudt_custodian = match self.sudt_custodians.get_mut(&sudt_type_hash) {
                Some(custodian) => custodian,
                None => bail!("withdrawal sudt {:x} not found", sudt_type_hash.pack()),
            };

            match sudt_custodian.balance.checked_sub(amount) {
                Some(remaind) => sudt_custodian.balance = remaind,
                None => bail!("withdrawal sudt {:x} overflow", sudt_type_hash.pack()),
            }

            // Consume all remaind sudt, give sudt custodian capacity back to ckb custodian
            if 0 == sudt_custodian.balance {
                if 0 == ckb_custodian.capacity {
                    ckb_custodian.capacity = sudt_custodian.capacity as u128;
                    ckb_custodian.balance =
                        (sudt_custodian.capacity - ckb_custodian.min_capacity) as u128;
                } else {
                    ckb_custodian.capacity += sudt_custodian.capacity as u128;
                    ckb_custodian.balance += sudt_custodian.capacity as u128;
                }
                sudt_custodian.capacity = 0;
            }
        }

        let ckb_amount = withdrawals_amount.capacity;
        match ckb_custodian.balance.checked_sub(ckb_amount) {
            Some(remaind) => {
                ckb_custodian.capacity -= ckb_amount;
                ckb_custodian.balance = remaind;
            }
            // Consume all remaind ckb
            None if ckb_amount == ckb_custodian.capacity => {
                ckb_custodian.capacity = 0;
                ckb_custodian.balance = 0;
            }
            None => bail!("withdrawal capacity overflow"),
        }

        Ok(())
    }

    fn finish(self) -> Vec<(CellOutput, Bytes)> {
        let mut outputs = Vec::with_capacity(self.sudt_custodians.len() + 1);
        let custodian_lock = build_finalized_custodian_lock(self.rollup_context);

        // Generate sudt custodian changes
        let sudt_changes = {
            let custodians = self.sudt_custodians.into_iter();
            custodians.filter(|(_, custodian)| 0 != custodian.capacity && 0 != custodian.balance)
        };
        for custodian in sudt_changes.map(|(_, c)| c) {
            let output = CellOutput::new_builder()
                .capacity(custodian.capacity.pack())
                .type_(Some(custodian.script).pack())
                .lock(custodian_lock.clone())
                .build();

            outputs.push((output, custodian.balance.pack().as_bytes()));
        }

        // Generate ckb custodian change
        let build_ckb_output = |capacity: u64| -> (CellOutput, Bytes) {
            let output = CellOutput::new_builder()
                .capacity(capacity.pack())
                .lock(custodian_lock.clone())
                .build();
            (output, Bytes::new())
        };
        if 0 != self.ckb_custodian.capacity {
            if self.ckb_custodian.capacity < u64::MAX as u128 {
                outputs.push(build_ckb_output(self.ckb_custodian.capacity as u64));
                return outputs;
            }

            // Fit ckb-indexer output_capacity_range [inclusive, exclusive]
            let max_capacity = u64::MAX - 1;
            let ckb_custodian = self.ckb_custodian;
            let mut remaind = ckb_custodian.capacity;
            while remaind > 0 {
                let max = remaind.saturating_sub(ckb_custodian.min_capacity as u128);
                match max.checked_sub(max_capacity as u128) {
                    Some(cap) => {
                        outputs.push(build_ckb_output(max_capacity));
                        remaind = cap.saturating_add(ckb_custodian.min_capacity as u128);
                    }
                    None if max.saturating_add(ckb_custodian.min_capacity as u128)
                        > max_capacity as u128 =>
                    {
                        let max = max.saturating_add(ckb_custodian.min_capacity as u128);
                        let half = max / 2;
                        outputs.push(build_ckb_output(half as u64));
                        outputs.push(build_ckb_output(max.saturating_sub(half) as u64));
                        remaind = 0;
                    }
                    None => {
                        outputs.push(build_ckb_output(
                            (max as u64).saturating_add(ckb_custodian.min_capacity),
                        ));
                        remaind = 0;
                    }
                }
            }
        }

        outputs
    }
}
