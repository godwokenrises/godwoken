#![allow(clippy::mutable_key_type)]

use std::collections::HashSet;

use anyhow::{anyhow, bail, Result};
use gw_rpc_client::{
    indexer_types::{Order, SearchKey, SearchKeyFilter},
    rpc_client::{QueryResult, RPCClient},
};
use gw_types::core::Timepoint;
use gw_types::offchain::CompatibleFinalizedTimepoint;
use gw_types::{
    core::ScriptHashType,
    offchain::{CellInfo, CollectedCustodianCells},
    packed::{CustodianLockArgsReader, Script},
    prelude::*,
};
use gw_utils::local_cells::{
    collect_local_and_indexer_cells, CollectLocalAndIndexerCursor, LocalCellsManager,
};

pub const MAX_CUSTODIANS: usize = 50;

pub async fn query_mergeable_custodians(
    local_cells_manager: &LocalCellsManager,
    rpc_client: &RPCClient,
    collected_custodians: CollectedCustodianCells,
    compatible_finalized_timepoint: &CompatibleFinalizedTimepoint,
) -> Result<QueryResult<CollectedCustodianCells>> {
    if collected_custodians.cells_info.len() >= MAX_CUSTODIANS {
        return Ok(QueryResult::Full(collected_custodians));
    }

    let query_result = query_mergeable_ckb_custodians(
        local_cells_manager,
        rpc_client,
        collected_custodians,
        compatible_finalized_timepoint,
        MAX_CUSTODIANS,
    )
    .await?;
    if matches!(query_result, QueryResult::Full(_)) {
        return Ok(query_result);
    }

    query_mergeable_sudt_custodians(
        rpc_client,
        query_result.expect_any(),
        compatible_finalized_timepoint,
        local_cells_manager,
    )
    .await
}

async fn query_mergeable_sudt_custodians(
    rpc_client: &RPCClient,
    collected: CollectedCustodianCells,
    compatible_finalized_timepoint: &CompatibleFinalizedTimepoint,
    local_cells_manager: &LocalCellsManager,
) -> Result<QueryResult<CollectedCustodianCells>> {
    if collected.cells_info.len() >= MAX_CUSTODIANS {
        return Ok(QueryResult::Full(collected));
    }

    query_mergeable_sudt_custodians_cells(
        local_cells_manager,
        rpc_client,
        collected,
        compatible_finalized_timepoint,
        MAX_CUSTODIANS,
    )
    .await
}

async fn query_mergeable_ckb_custodians(
    local_cells_manager: &LocalCellsManager,
    rpc_client: &RPCClient,
    mut collected: CollectedCustodianCells,
    compatible_finalized_timepoint: &CompatibleFinalizedTimepoint,
    max_cells: usize,
) -> Result<QueryResult<CollectedCustodianCells>> {
    const MIN_MERGE_CELLS: usize = 5;
    log::debug!("ckb merge MIN_MERGE_CELLS {}", MIN_MERGE_CELLS);

    let remain = max_cells.saturating_sub(collected.cells_info.len());
    if remain < MIN_MERGE_CELLS {
        log::debug!("ckb merge break remain < `MIN_MERGE_CELLS`");
        return Ok(QueryResult::NotEnough(collected));
    }

    let rollup_config = &rpc_client.rollup_config;
    let rollup_type_script = &rpc_client.rollup_type_script;
    let custodian_lock = Script::new_builder()
        .code_hash(rollup_config.custodian_script_type_hash())
        .hash_type(ScriptHashType::Type.into())
        .args(rollup_type_script.calc_script_hash().as_bytes().pack())
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
            if !compatible_finalized_timepoint.is_finalized(&Timepoint::from_full_value(
                custodian_lock_args_reader.deposit_block_number().unpack(),
            )) {
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
    compatible_finalized_timepoint: &CompatibleFinalizedTimepoint,
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
        .query_random_sudt_type_script(compatible_finalized_timepoint, MAX_MERGE_SUDTS)
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
                compatible_finalized_timepoint,
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
