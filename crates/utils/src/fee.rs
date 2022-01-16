#![allow(clippy::mutable_key_type)]

use crate::transaction_skeleton::TransactionSkeleton;
use anyhow::Result;
use gw_rpc_client::indexer_client::CKBIndexerClient;
use gw_types::{
    offchain::InputCellInfo,
    packed::{CellInput, CellOutput, Script},
    prelude::*,
};

/// Calculate tx fee
/// TODO accept fee rate args
fn calculate_required_tx_fee(tx_size: usize) -> u64 {
    // tx_size * KB / MIN_FEE_RATE
    tx_size as u64
}

/// Add fee cell to tx skeleton
pub async fn fill_tx_fee(
    tx_skeleton: &mut TransactionSkeleton,
    client: &CKBIndexerClient,
    lock_script: Script,
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
        let cells = client
            .query_payment_cells(lock_script.clone(), required_fee, &taken_outpoints)
            .await?;
        assert!(!cells.is_empty(), "need cells to pay fee");

        // put cells in tx skeleton
        tx_skeleton
            .inputs_mut()
            .extend(cells.into_iter().map(|cell| {
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
