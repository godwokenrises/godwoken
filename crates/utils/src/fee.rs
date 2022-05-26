#![allow(clippy::mutable_key_type)]

use std::collections::HashSet;

use crate::{local_cells::LocalCellsManager, transaction_skeleton::TransactionSkeleton};
use anyhow::{bail, Result};
use gw_rpc_client::{
    indexer_client::CKBIndexerClient,
    indexer_types::{Order, ScriptType, SearchKey},
};
use gw_types::{
    offchain::{CellInfo, InputCellInfo},
    packed::{CellInput, CellOutput, OutPoint, Script},
    prelude::*,
};

/// Calculate tx fee
/// TODO accept fee rate args
fn calculate_required_tx_fee(tx_size: usize) -> u64 {
    // tx_size * KB / MIN_FEE_RATE
    tx_size as u64
}

/// Add fee cell to tx skeleton
pub async fn fill_tx_fee_with_local(
    tx_skeleton: &mut TransactionSkeleton,
    client: &CKBIndexerClient,
    lock_script: Script,
    local_cells_manager: Option<&LocalCellsManager>,
) -> Result<()> {
    const CHANGE_CELL_CAPACITY: u64 = 61_00000000;

    let estimate_tx_size_with_change = |tx_skeleton: &mut TransactionSkeleton| -> Result<usize> {
        let change_cell = CellOutput::new_builder()
            .lock(lock_script.clone())
            .capacity(CHANGE_CELL_CAPACITY.pack())
            .build();

        tx_skeleton
            .outputs_mut()
            .push((change_cell, Default::default()));

        let tx_size = tx_skeleton.tx_in_block_size()?;
        tx_skeleton.outputs_mut().pop();

        Ok(tx_size)
    };

    // calculate required fee
    // Try to generate a change output cell. If input cannot cover fee, query an owner cell.
    let tx_size = estimate_tx_size_with_change(tx_skeleton)?;
    let tx_fee = calculate_required_tx_fee(tx_size);
    let max_paid_fee = tx_skeleton
        .calculate_fee()?
        .saturating_sub(CHANGE_CELL_CAPACITY);

    let mut required_fee = tx_fee.saturating_sub(max_paid_fee);
    if 0 == required_fee {
        let change_capacity = max_paid_fee + CHANGE_CELL_CAPACITY - tx_fee;
        let change_cell = CellOutput::new_builder()
            .lock(lock_script.clone())
            .capacity(change_capacity.pack())
            .build();

        tx_skeleton
            .outputs_mut()
            .push((change_cell, Default::default()));

        return Ok(());
    }

    required_fee += CHANGE_CELL_CAPACITY;

    let mut change_capacity = 0;
    while required_fee > 0 {
        // to filter used input cells
        let taken_outpoints = tx_skeleton.taken_outpoints()?;
        // get payment cells
        let cells = collect_payment_cells(
            client,
            lock_script.clone(),
            required_fee,
            &taken_outpoints,
            local_cells_manager,
        )
        .await?;
        assert!(!cells.is_empty(), "need cells to pay fee");

        // put cells in tx skeleton
        tx_skeleton
            .inputs_mut()
            .extend(cells.into_iter().map(|cell| {
                log::info!("using payment cell {:?}", cell.out_point);
                let input = CellInput::new_builder()
                    .previous_output(cell.out_point.clone())
                    .build();
                InputCellInfo { input, cell }
            }));

        let tx_size = estimate_tx_size_with_change(tx_skeleton)?;
        let tx_fee = calculate_required_tx_fee(tx_size);
        let max_paid_fee = tx_skeleton
            .calculate_fee()?
            .saturating_sub(CHANGE_CELL_CAPACITY);

        required_fee = tx_fee.saturating_sub(max_paid_fee);
        change_capacity = max_paid_fee + CHANGE_CELL_CAPACITY - tx_fee;
    }

    let change_cell = CellOutput::new_builder()
        .lock(lock_script)
        .capacity(change_capacity.pack())
        .build();

    tx_skeleton
        .outputs_mut()
        .push((change_cell, Default::default()));

    Ok(())
}

/// Add fee cell to tx skeleton
// TODO: always use fill_tx_fee_with_local.
pub async fn fill_tx_fee(
    tx_skeleton: &mut TransactionSkeleton,
    client: &CKBIndexerClient,
    lock_script: Script,
) -> Result<()> {
    fill_tx_fee_with_local(tx_skeleton, client, lock_script, None).await
}

/// query payment cells, the returned cells should provide at least required_capacity fee,
/// and the remained fees should be enough to cover a charge cell
pub async fn collect_payment_cells(
    client: &CKBIndexerClient,
    lock: Script,
    required_capacity: u64,
    taken_outpoints: &HashSet<OutPoint>,
    local_cells_manager: Option<&LocalCellsManager>,
) -> Result<Vec<CellInfo>> {
    let mut collected_cells = Vec::new();
    let mut collected_capacity = 0u64;

    // First try local live cells. Usually this is enough.
    for cell in local_cells_manager
        .iter()
        .flat_map(|l| l.local_live())
        .filter(|c| c.output.lock() == lock && c.data.is_empty() && c.output.type_().is_none())
    {
        collected_cells.push(cell.clone());
        collected_capacity = collected_capacity.saturating_add(cell.output.capacity().unpack());
        if collected_capacity >= required_capacity {
            return Ok(collected_cells);
        }
    }

    let search_key = SearchKey {
        script: {
            let lock = ckb_types::packed::Script::new_unchecked(lock.as_bytes());
            lock.into()
        },
        script_type: ScriptType::Lock,
        filter: None,
    };
    let order = Order::Desc;
    let mut cursor = None;

    while collected_capacity < required_capacity {
        let cells = client.get_cells(&search_key, &order, None, &cursor).await?;

        if cells.last_cursor.is_empty() {
            bail!(
                "no enough payment cells, required: {}, taken: {:?}",
                required_capacity,
                taken_outpoints
            );
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
                || local_cells_manager.map_or(false, |l| l.is_dead(&out_point))
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
            collected_capacity = collected_capacity.saturating_add(cell.output.capacity().unpack());
            collected_cells.push(cell);
            if collected_capacity >= required_capacity {
                break;
            }
        }
    }
    Ok(collected_cells)
}
