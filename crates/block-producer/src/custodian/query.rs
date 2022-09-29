// Move from mem_pool/custodian.rs, now only block producer use these query.

#![allow(clippy::mutable_key_type)]

use std::{collections::HashSet, time::Instant};

use anyhow::{anyhow, bail, Result};
use gw_common::{CKB_SUDT_SCRIPT_ARGS, H256};
use gw_rpc_client::{
    indexer_client::CKBIndexerClient,
    indexer_types::{Order, SearchKey, SearchKeyFilter},
    rpc_client::{QueryResult, RPCClient},
};
use gw_store::traits::chain_store::ChainStore;
use gw_types::{
    core::ScriptHashType,
    offchain::{CellInfo, CollectedCustodianCells},
    packed::{CustodianLockArgsReader, Script, WithdrawalRequest},
    prelude::*,
};
use gw_utils::{
    custodian::{calc_ckb_custodian_min_capacity, generate_finalized_custodian},
    local_cells::{
        collect_local_and_indexer_cells, CollectLocalAndIndexerCursor, LocalCellsManager,
    },
    withdrawal::sum_withdrawals,
};

use gw_types::offchain::{RollupContext, WithdrawalsAmount};
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

#[instrument(skip_all, fields(last_finalized_block_number = last_finalized_block_number))]
pub async fn query_finalized_custodians<WithdrawalIter: Iterator<Item = WithdrawalRequest>>(
    rpc_client: &RPCClient,
    db: &impl ChainStore,
    withdrawals: WithdrawalIter,
    rollup_context: &RollupContext,
    last_finalized_block_number: u64,
    local_cells_manager: &LocalCellsManager,
) -> Result<QueryResult<CollectedCustodianCells>> {
    let total_withdrawal_amount = sum_withdrawals(withdrawals);
    let total_change_capacity = sum_change_capacity(db, rollup_context, &total_withdrawal_amount);

    query_finalized_custodian_cells(
        local_cells_manager,
        &rpc_client.indexer,
        rollup_context,
        &total_withdrawal_amount,
        total_change_capacity,
        last_finalized_block_number,
        None,
        MAX_CUSTODIANS,
    )
    .await
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

async fn query_mergeable_sudt_custodians_cells(
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

#[instrument(skip_all, fields(withdrawals_amount = ?withdrawals_amount))]
fn sum_change_capacity(
    db: &impl ChainStore,
    rollup_context: &RollupContext,
    withdrawals_amount: &WithdrawalsAmount,
) -> u128 {
    let to_change_capacity = |sudt_script_hash: &[u8; 32]| -> u128 {
        match db.get_asset_script(&H256::from(*sudt_script_hash)) {
            Ok(Some(script)) => {
                let (change, _data) = generate_finalized_custodian(rollup_context, 1, script);
                change.capacity().unpack() as u128
            }
            _ => {
                let hex = hex::encode(&sudt_script_hash);
                log::warn!("unknown sudt script hash {:?}", hex);
                0
            }
        }
    };

    let ckb_change_capacity = calc_ckb_custodian_min_capacity(rollup_context) as u128;
    let sudt_change_capacity: u128 = {
        let sudt_script_hashes = withdrawals_amount.sudt.keys();
        sudt_script_hashes.map(to_change_capacity).sum()
    };

    ckb_change_capacity + sudt_change_capacity
}

#[allow(clippy::too_many_arguments)]
async fn query_finalized_custodian_cells(
    local_cells_manager: &LocalCellsManager,
    indexer: &CKBIndexerClient,
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

    let parse_sudt_amount = |cell: &CellInfo| -> Result<u128> {
        if cell.output.type_().is_none() {
            bail!("no a sudt cell");
        }

        gw_types::packed::Uint128::from_slice(cell.data.as_ref())
            .map(|a| a.unpack())
            .map_err(|e| anyhow!("invalid sudt amount {}", e))
    };

    let custodian_lock = Script::new_builder()
        .code_hash(rollup_context.rollup_config.custodian_script_type_hash())
        .hash_type(ScriptHashType::Type.into())
        .args(rollup_context.rollup_script_hash.as_slice().pack())
        .build();
    let filter = min_capacity.map(|min_capacity| SearchKeyFilter {
        output_capacity_range: Some([min_capacity.into(), u64::MAX.into()]), // [inclusive, exclusive]
        ..Default::default()
    });

    let search_key = SearchKey::with_lock(custodian_lock).with_filter(filter);
    // order by ASC so we can search more cells
    let order = Order::Asc;

    let mut collected = CollectedCustodianCells::default();
    let mut collected_fullfilled_sudt = HashSet::new();
    let mut cursor = CollectLocalAndIndexerCursor::Local;

    // withdrawal ckb + change custodian capacity
    let required_capacity = {
        let withdrawal_capacity = withdrawals_amount.capacity;
        withdrawal_capacity.saturating_add(custodian_change_capacity)
    };

    while collected.capacity < required_capacity
        || collected_fullfilled_sudt.len() < withdrawals_amount.sudt.len()
    {
        let cells = collect_local_and_indexer_cells(
            local_cells_manager,
            indexer,
            &search_key,
            &order,
            None,
            &mut cursor,
        )
        .await?;

        if cursor.is_ended() {
            return Ok(QueryResult::NotEnough(collected));
        }

        query_indexer_times += 1;
        query_indexer_cells += cells.len();

        for cell in cells {
            if collected.cells_info.len() >= max_cells {
                return Ok(QueryResult::NotEnough(collected));
            }

            // Skip ckb custodians if capacity is fullfill
            if collected.capacity >= required_capacity
                && !withdrawals_amount.sudt.is_empty()
                && cell.output.type_().is_none()
            {
                continue;
            }

            let args = cell.output.as_reader().lock().args().raw_data();
            let custodian_lock_args = match CustodianLockArgsReader::from_slice(&args[32..]) {
                Ok(r) => r,
                Err(_) => continue,
            };

            if custodian_lock_args.deposit_block_number().unpack() > last_finalized_block_number {
                continue;
            }

            // Collect sudt
            if let Some(sudt_type_script) = cell.output.type_().to_opt() {
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
                        collected
                            .sudt
                            .entry(sudt_type_hash)
                            .or_insert((0, Script::default()))
                    };
                    *collected_amount = collected_amount.saturating_add(sudt_amount);
                    *type_script = sudt_type_script;

                    if *collected_amount >= *withdrawal_amount {
                        collected_fullfilled_sudt.insert(sudt_type_hash);
                    }
                }
            }

            collected.capacity = collected
                .capacity
                .saturating_add(cell.output.capacity().unpack().into());

            collected.cells_info.push(cell);

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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use gw_rpc_client::indexer_client::CKBIndexerClient;
    use gw_rpc_client::rpc_client::QueryResult;
    use gw_types::bytes::Bytes;
    use gw_types::core::ScriptHashType;
    use gw_types::offchain::{CellInfo, RollupContext, WithdrawalsAmount};
    use gw_types::packed::{
        CellOutput, CustodianLockArgs, OutPoint, RollupConfig, Script, Uint128,
    };
    use gw_types::prelude::{Builder, Entity, Pack, Unpack};
    use gw_utils::local_cells::LocalCellsManager;

    const CKB: u64 = 100_000_000;

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

        let mut local_cells_manager = LocalCellsManager::default();
        for c in ten_ckb_cells.into_iter().chain(one_sudt_cell) {
            local_cells_manager.add_live(c);
        }

        let indexer_client = CKBIndexerClient::with_url("http://host.invalid").unwrap();

        let result = super::query_finalized_custodian_cells(
            &local_cells_manager,
            &indexer_client,
            &rollup_context,
            &withdrawals_amount,
            change_capacity,
            FINALIZED_BLOCK_NUMBER,
            None,
            max_five_cells,
        )
        .await
        .unwrap();

        assert!(matches!(result, QueryResult::Full(_)));
    }

    fn generate_finalized_ckb_custodian_cells(
        cell_num: usize,
        rollup_context: &RollupContext,
        last_finalized_block_number: u64,
        capacity: u64,
    ) -> Vec<CellInfo> {
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
            .map(|i| CellInfo {
                output: output.clone(),
                data: Default::default(),
                out_point: OutPoint::new_builder().index((i as u32).pack()).build(),
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
    ) -> Vec<CellInfo> {
        let ckb_cells = generate_finalized_ckb_custodian_cells(
            cell_num,
            rollup_context,
            last_finalized_block_number,
            capacity,
        );

        let convert_to_sudt = |cell: CellInfo| {
            let output = cell
                .output
                .as_builder()
                .type_(Some(sudt_script.clone()).pack())
                .build();
            let mut idx: u32 = cell.out_point.index().unpack();
            idx += 10000;
            CellInfo {
                output,
                data: amount.as_bytes(),
                out_point: cell.out_point.as_builder().index(idx.pack()).build(),
            }
        };
        ckb_cells.into_iter().map(convert_to_sudt).collect()
    }
}
