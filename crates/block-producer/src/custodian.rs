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
};
use gw_types::{
    bytes::Bytes,
    offchain::{InputCellInfo, RollupContext, WithdrawalsAmount},
    packed::{CellInput, CellOutput},
};
use tracing::instrument;

pub const MAX_CUSTODIANS: usize = 50;
// Fit ckb-indexer output_capacity_range [inclusive, exclusive]
pub const MAX_CAPACITY: u64 = u64::MAX - 1;

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

#[derive(Debug)]
pub struct AggregatedCustodians {
    pub inputs: Vec<InputCellInfo>,
    pub outputs: Vec<(CellOutput, Bytes)>,
}

impl<'a> From<&'a CollectedCustodianCells> for AvailableCustodians {
    fn from(collected: &'a CollectedCustodianCells) -> Self {
        AvailableCustodians {
            capacity: collected.capacity,
            sudt: collected.sudt.clone(),
        }
    }
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

    let mut aggregator = Aggregator::from_custodians(rollup_context, available_custodians);
    aggregator.minus_withdrawals(withdrawals_amount)?;

    let custodian_inputs = finalized_custodians.cells_info.into_iter().map(|cell| {
        let input = CellInput::new_builder()
            .previous_output(cell.out_point.clone())
            .build();
        InputCellInfo { input, cell }
    });

    let aggregated = AggregatedCustodians {
        inputs: custodian_inputs.collect(),
        outputs: aggregator.generate_balance_outputs(),
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

#[derive(Debug, Clone, Default)]
pub struct AvailableCustodians {
    pub capacity: u128,
    pub sudt: HashMap<[u8; 32], (u128, Script)>,
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
    /// # Panics
    ///
    /// Panics if accumulate u64 capacity into u128 overflow
    fn from_custodians(
        rollup_context: &'a RollupContext,
        available_custodians: AvailableCustodians,
    ) -> Self {
        let mut total_sudt_min_occupied_capacity = 0u128;
        let mut sudt_custodians = HashMap::new();

        for (sudt_type_hash, (balance, type_script)) in available_custodians.sudt.into_iter() {
            let (change, _data) =
                generate_finalized_custodian(rollup_context, balance, type_script.clone());

            let sudt_custodian = SudtCustodian {
                capacity: change.capacity().unpack(),
                balance,
                script: type_script,
            };

            total_sudt_min_occupied_capacity = total_sudt_min_occupied_capacity
                .checked_add(sudt_custodian.capacity as u128)
                .expect("accumulate u64 capacity into u128 overflow");
            sudt_custodians.insert(sudt_type_hash, sudt_custodian);
        }

        // NOTE: Use `saturating_sub` because change sudt custodian may not be needed if
        // its amount reach to 0.
        let ckb_custodian_min_capacity = calc_ckb_custodian_min_capacity(rollup_context);
        let ckb_custodian_capacity = available_custodians
            .capacity
            .saturating_sub(total_sudt_min_occupied_capacity);
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
                debug_assert!(sudt_custodian.capacity > ckb_custodian.min_capacity);

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

    fn generate_balance_outputs(self) -> Vec<(CellOutput, Bytes)> {
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

            let ckb_custodian = self.ckb_custodian;
            let mut remaind = ckb_custodian.capacity;
            while remaind > 0 {
                let max = remaind.saturating_sub(ckb_custodian.min_capacity as u128);
                match max.checked_sub(MAX_CAPACITY as u128) {
                    Some(cap) => {
                        outputs.push(build_ckb_output(MAX_CAPACITY));
                        remaind = cap.saturating_add(ckb_custodian.min_capacity as u128);
                    }
                    None if max.saturating_add(ckb_custodian.min_capacity as u128)
                        > MAX_CAPACITY as u128 =>
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

#[cfg(test)]
mod tests {
    use gw_common::H256;
    use gw_types::{
        core::ScriptHashType,
        offchain::CellInfo,
        packed::{OutPoint, RollupConfig},
    };

    use super::*;

    const CKB: u128 = 10u128.pow(8);

    macro_rules! assert_output {
        ($a:expr, $b:expr) => {
            assert_eq!($a.0.as_slice(), $b.0.as_slice());
            assert_eq!($a.1, $b.1)
        };
    }

    fn sample_rollup_context() -> RollupContext {
        let rollup_script_hash: H256 = [1u8; 32].into();
        let custodian_script_type_hash: H256 = [2u8; 32].into();
        let l1_sudt_script_type_hash: H256 = [3u8; 32].into();

        RollupContext {
            rollup_script_hash,
            rollup_config: RollupConfig::new_builder()
                .l1_sudt_script_type_hash(l1_sudt_script_type_hash.pack())
                .custodian_script_type_hash(custodian_script_type_hash.pack())
                .build(),
        }
    }

    #[test]
    fn test_aggregate_balance() {
        const AVAILABLE_CAPACITY: u128 = 1000 * CKB;
        const SUDT_AMOUNT: u128 = 1000;
        const WITHDRAWAL_CAPACITY: u128 = 100 * CKB;

        let rollup_context = sample_rollup_context();
        let sudt_a = Script::new_builder()
            .code_hash(rollup_context.rollup_config.l1_sudt_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(rand::random::<[u8; 32]>().to_vec().pack())
            .build();

        let cell_info = {
            let out_point = OutPoint::new_builder()
                .tx_hash(rand::random::<[u8; 32]>().pack())
                .build();

            let (output, data) =
                generate_finalized_custodian(&rollup_context, SUDT_AMOUNT, sudt_a.clone());
            let output = output
                .as_builder()
                .capacity((AVAILABLE_CAPACITY as u64).pack())
                .build();

            CellInfo {
                out_point,
                output,
                data,
            }
        };

        let finalized_custodians = CollectedCustodianCells {
            cells_info: vec![cell_info.clone()],
            capacity: AVAILABLE_CAPACITY,
            sudt: HashMap::from([(sudt_a.hash(), (SUDT_AMOUNT, sudt_a.clone()))]),
        };

        let withdrawals_amount = WithdrawalsAmount {
            capacity: WITHDRAWAL_CAPACITY,
            sudt: HashMap::from([(sudt_a.hash(), 1)]),
        };

        let AggregatedCustodians { inputs, outputs } =
            aggregate_balance(&rollup_context, finalized_custodians, withdrawals_amount)
                .unwrap()
                .unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(outputs.len(), 2);

        let first_input = inputs.first().unwrap();
        let expect_input = {
            let input = CellInput::new_builder()
                .previous_output(cell_info.out_point.clone())
                .build();

            InputCellInfo {
                input,
                cell: cell_info,
            }
        };
        assert_eq!(first_input.input.as_slice(), expect_input.input.as_slice());
        assert_eq!(
            first_input.cell.out_point.as_slice(),
            expect_input.cell.out_point.as_slice()
        );
        assert_eq!(
            first_input.cell.output.as_slice(),
            expect_input.cell.output.as_slice()
        );
        assert_eq!(first_input.cell.data, expect_input.cell.data,);

        let sudt_output = outputs.first().unwrap();
        let ckb_output = outputs.get(1).unwrap();

        let expected_sudt_a_output =
            generate_finalized_custodian(&rollup_context, SUDT_AMOUNT - 1, sudt_a);

        let sudt_occupied_capacity = expected_sudt_a_output.0.capacity().unpack();
        let expected_ckb_output = ckb_finalized_custodian(
            &rollup_context,
            (AVAILABLE_CAPACITY - WITHDRAWAL_CAPACITY) as u64 - sudt_occupied_capacity,
        );
        assert_output!(sudt_output, expected_sudt_a_output);
        assert_output!(ckb_output, expected_ckb_output);

        // nothing to aggregate
        let maybe_balance = aggregate_balance(
            &rollup_context,
            CollectedCustodianCells::default(),
            WithdrawalsAmount::default(),
        );
        assert!(maybe_balance.unwrap().is_none());

        let maybe_balance = aggregate_balance(
            &rollup_context,
            CollectedCustodianCells {
                cells_info: vec![CellInfo::default()],
                ..Default::default()
            },
            WithdrawalsAmount::default(),
        );
        assert!(maybe_balance.unwrap().is_none());
    }

    #[test]
    fn test_aggregator() {
        const AVAILABLE_CAPACITY: u128 = 1000 * CKB;
        const WITHDRAWAL_CAPACITY: u128 = 100 * CKB;

        let rollup_context = sample_rollup_context();

        let sudt_a = Script::new_builder()
            .code_hash(rollup_context.rollup_config.l1_sudt_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(rand::random::<[u8; 32]>().to_vec().pack())
            .build();

        let sudt_b = Script::new_builder()
            .code_hash(rollup_context.rollup_config.l1_sudt_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(rand::random::<[u8; 32]>().to_vec().pack())
            .build();

        let custodians = AvailableCustodians {
            capacity: AVAILABLE_CAPACITY,
            sudt: HashMap::from([
                (sudt_a.hash(), (1000, sudt_a.clone())),
                (sudt_b.hash(), (999, sudt_b.clone())),
            ]),
        };
        let withdrawals_amount = WithdrawalsAmount {
            capacity: WITHDRAWAL_CAPACITY,
            sudt: HashMap::from([(sudt_a.hash(), 1), (sudt_b.hash(), 2)]),
        };

        let mut aggregator = Aggregator::from_custodians(&rollup_context, custodians);
        aggregator.minus_withdrawals(withdrawals_amount).unwrap();

        let custodian_outputs = aggregator.generate_balance_outputs();
        assert_eq!(custodian_outputs.len(), 3);

        let expected_sudt_a_output = generate_finalized_custodian(&rollup_context, 999, sudt_a);
        let expected_sudt_b_output = generate_finalized_custodian(&rollup_context, 997, sudt_b);

        let sudt_occupied_capacity = expected_sudt_a_output.0.capacity().unpack();
        let expected_ckb_output = ckb_finalized_custodian(
            &rollup_context,
            (AVAILABLE_CAPACITY - WITHDRAWAL_CAPACITY) as u64 - sudt_occupied_capacity * 2,
        );

        let mut first_sudt_output = custodian_outputs.first().unwrap();
        let mut second_sudt_output = custodian_outputs.get(1).unwrap();
        if first_sudt_output.0.type_().as_slice() == expected_sudt_b_output.0.type_().as_slice() {
            std::mem::swap(&mut first_sudt_output, &mut second_sudt_output);
        }
        assert_output!(first_sudt_output, expected_sudt_a_output);
        assert_output!(second_sudt_output, expected_sudt_b_output);

        let ckb_output = custodian_outputs.get(2).unwrap();
        assert_output!(ckb_output, expected_ckb_output);
    }

    #[test]
    #[ignore = "accumulate u64 capacity into u128 overflow"]
    fn test_aggregator_accumulate_u64_capacity_into_u128_overflow() {
        unreachable!()
    }

    #[test]
    fn test_aggregator_minus_withdrawals_no_change_custodian() {
        const AVAILABLE_CAPACITY: u128 = 800 * CKB;
        const WITHDRAWAL_CAPACITY: u128 = 100 * CKB;

        let rollup_context = sample_rollup_context();

        let sudt_a = Script::new_builder()
            .code_hash(rollup_context.rollup_config.l1_sudt_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(rand::random::<[u8; 32]>().to_vec().pack())
            .build();

        let (output, _data) = generate_finalized_custodian(&rollup_context, 1, sudt_a.clone());
        let min_sudt_occupied_capacity = output.capacity().unpack();

        // Consume all sudt custodian (aggregator ckb capacity isn't zero)
        let custodians = AvailableCustodians {
            capacity: AVAILABLE_CAPACITY,
            sudt: HashMap::from([(sudt_a.hash(), (1, sudt_a.clone()))]),
        };

        let withdrawals_amount = WithdrawalsAmount {
            capacity: WITHDRAWAL_CAPACITY,
            sudt: HashMap::from([(sudt_a.hash(), 1)]),
        };

        let mut aggregator = Aggregator::from_custodians(&rollup_context, custodians);
        aggregator.minus_withdrawals(withdrawals_amount).unwrap();

        assert_eq!(
            aggregator.ckb_custodian.capacity,
            (AVAILABLE_CAPACITY - WITHDRAWAL_CAPACITY),
        );
        assert_eq!(
            aggregator.ckb_custodian.balance,
            (AVAILABLE_CAPACITY - WITHDRAWAL_CAPACITY)
                - aggregator.ckb_custodian.min_capacity as u128,
        );

        let custodian_outputs = aggregator.generate_balance_outputs();
        assert_eq!(custodian_outputs.len(), 1);

        let expected_ckb_output = ckb_finalized_custodian(
            &rollup_context,
            (AVAILABLE_CAPACITY - WITHDRAWAL_CAPACITY) as u64,
        );

        let ckb_output = custodian_outputs.first().unwrap();
        assert_output!(ckb_output, expected_ckb_output);

        // Consume all sudt custodian (aggregator zero ckb capaicty)
        let custodians = AvailableCustodians {
            capacity: min_sudt_occupied_capacity as u128,
            sudt: HashMap::from([(sudt_a.hash(), (1, sudt_a.clone()))]),
        };

        let withdrawals_amount = WithdrawalsAmount {
            capacity: 0,
            sudt: HashMap::from([(sudt_a.hash(), 1)]),
        };

        let mut aggregator = Aggregator::from_custodians(&rollup_context, custodians);
        aggregator.minus_withdrawals(withdrawals_amount).unwrap();

        assert_eq!(
            aggregator.ckb_custodian.capacity,
            min_sudt_occupied_capacity as u128
        );
        assert_eq!(
            aggregator.ckb_custodian.balance,
            (min_sudt_occupied_capacity - aggregator.ckb_custodian.min_capacity) as u128
        );

        let custodian_outputs = aggregator.generate_balance_outputs();
        assert_eq!(custodian_outputs.len(), 1);

        let expected_ckb_output =
            ckb_finalized_custodian(&rollup_context, min_sudt_occupied_capacity as u64);

        let ckb_output = custodian_outputs.first().unwrap();
        assert_output!(ckb_output, expected_ckb_output);

        // Consume all ckb custodian
        let custodians = AvailableCustodians {
            capacity: min_sudt_occupied_capacity as u128,
            sudt: HashMap::from([(sudt_a.hash(), (1, sudt_a.clone()))]),
        };

        let withdrawals_amount = WithdrawalsAmount {
            capacity: min_sudt_occupied_capacity as u128,
            sudt: HashMap::from([(sudt_a.hash(), 1)]),
        };

        let mut aggregator = Aggregator::from_custodians(&rollup_context, custodians);
        aggregator.minus_withdrawals(withdrawals_amount).unwrap();

        let custodian_outputs = aggregator.generate_balance_outputs();
        assert_eq!(custodian_outputs.len(), 0);
    }

    #[test]
    fn test_aggregator_invalid_minus_withdrawal() {
        let rollup_context = sample_rollup_context();

        let sudt_a = Script::new_builder()
            .code_hash(rollup_context.rollup_config.l1_sudt_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(rand::random::<[u8; 32]>().to_vec().pack())
            .build();

        let sudt_b = Script::new_builder()
            .code_hash(rollup_context.rollup_config.l1_sudt_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(rand::random::<[u8; 32]>().to_vec().pack())
            .build();

        let custodians = AvailableCustodians {
            capacity: 800 * CKB,
            sudt: HashMap::from([(sudt_a.hash(), (1, sudt_a.clone()))]),
        };

        let mut aggregator = Aggregator::from_custodians(&rollup_context, custodians);

        // sudt not found
        let withdrawals_amount = WithdrawalsAmount {
            capacity: 100 * CKB,
            sudt: HashMap::from([(sudt_b.hash(), 1)]),
        };
        let err = aggregator
            .minus_withdrawals(withdrawals_amount)
            .unwrap_err();
        assert!(err.to_string().contains("not found"));

        // sudt overflow
        let withdrawals_amount = WithdrawalsAmount {
            capacity: 100 * CKB,
            sudt: HashMap::from([(sudt_a.hash(), 2)]),
        };
        let err = aggregator
            .minus_withdrawals(withdrawals_amount)
            .unwrap_err();
        assert!(err.to_string().contains("overflow"));

        // capacity overflow
        let withdrawals_amount = WithdrawalsAmount {
            capacity: 900 * CKB,
            sudt: HashMap::new(),
        };
        let err = aggregator
            .minus_withdrawals(withdrawals_amount)
            .unwrap_err();
        assert!(err.to_string().contains("withdrawal capacity overflow"));
    }

    #[test]
    fn test_aggregator_generate_balance_outputs_split_u64_max_ckb_custodians_capacity() {
        let rollup_context = sample_rollup_context();

        // Split with MAX_CAPACITY
        let available_capacity = (MAX_CAPACITY as u128 * 2) + 300 * CKB;
        let custodians = AvailableCustodians {
            capacity: available_capacity,
            sudt: HashMap::new(),
        };

        let aggregator = Aggregator::from_custodians(&rollup_context, custodians);
        let outputs = aggregator.generate_balance_outputs();
        assert_eq!(outputs.len(), 3);

        let first_output = outputs.first().unwrap();
        let second_output = outputs.get(1).unwrap();
        let third_output = outputs.get(2).unwrap();

        let expected_max_capacity_output = ckb_finalized_custodian(&rollup_context, MAX_CAPACITY);
        let expected_rest_output = ckb_finalized_custodian(
            &rollup_context,
            (available_capacity - MAX_CAPACITY as u128 * 2) as u64,
        );
        assert_output!(first_output, expected_max_capacity_output);
        assert_output!(second_output, expected_max_capacity_output);
        assert_output!(third_output, expected_rest_output);

        // Split into half
        let available_capacity = MAX_CAPACITY as u128 + 2 * CKB;
        let custodians = AvailableCustodians {
            capacity: available_capacity,
            sudt: HashMap::new(),
        };

        let aggregator = Aggregator::from_custodians(&rollup_context, custodians);
        let outputs = aggregator.generate_balance_outputs();
        assert_eq!(outputs.len(), 2);

        let first_output = outputs.first().unwrap();
        let second_output = outputs.get(1).unwrap();

        let half = available_capacity / 2;
        let expected_first_output = ckb_finalized_custodian(&rollup_context, half as u64);
        let expected_second_output =
            ckb_finalized_custodian(&rollup_context, (available_capacity - half) as u64);
        assert_output!(first_output, expected_first_output);
        assert_output!(second_output, expected_second_output);
    }

    fn ckb_finalized_custodian(
        rollup_context: &RollupContext,
        capacity: u64,
    ) -> (CellOutput, Bytes) {
        let custodian_lock = build_finalized_custodian_lock(rollup_context);

        let output = CellOutput::new_builder()
            .capacity(capacity.pack())
            .lock(custodian_lock)
            .build();

        (output, Bytes::new())
    }
}
