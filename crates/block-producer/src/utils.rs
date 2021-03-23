use std::convert::TryInto;

use crate::{cell_collector::CellCollector, transaction_skeleton::TransactionSkeleton};
use anyhow::{anyhow, Result};
use gw_types::{packed::CellInput, prelude::*};

/// 100 shannons per KB
const MIN_FEE_RATE: usize = 1000;
const KB: usize = 1000;

/// Calculate tx fee
fn calculate_required_tx_fee(tx_size: usize) -> u64 {
    // tx_size * KB / MIN_FEE_RATE
    tx_size as u64
}

/// calculate tx skeleton inputs / outputs
fn calculate_paid_fee(
    tx_skeleton: &TransactionSkeleton,
    cell_collector: &CellCollector,
) -> Result<(u128, u128)> {
    let mut input_capacity: u128 = 0;
    for input in tx_skeleton.inputs() {
        let cell = cell_collector
            .get_cell(&input.previous_output())
            .ok_or(anyhow!("unknown input: {}", input))?;
        let capacity: u64 = cell.output.capacity().unpack();
        input_capacity = input_capacity
            .checked_add(capacity.into())
            .ok_or(anyhow!("overflow"))?;
    }

    let mut output_capacity: u128 = 0;
    for (output, _data) in tx_skeleton.outputs() {
        let capacity: u64 = output.capacity().unpack();
        output_capacity = output_capacity
            .checked_add(capacity.into())
            .ok_or(anyhow!("overflow"))?;
    }
    Ok((input_capacity, output_capacity))
}

/// Add fee cell to tx skeleton
pub fn fill_tx_fee(
    tx_skeleton: &mut TransactionSkeleton,
    cell_collector: &CellCollector,
    lock_hash: [u8; 32],
) -> Result<()> {
    let tx_size: usize = tx_skeleton.tx_in_block_size()?;
    let (input_capacity, output_capacity) = calculate_paid_fee(tx_skeleton, cell_collector)?;
    assert!(
        input_capacity >= output_capacity,
        "Rollup cells capacity should be enough to use"
    );
    let paid_fee: u64 = (input_capacity - output_capacity)
        .try_into()
        .expect("paid fee too large");
    // calculate required fee
    let required_fee = calculate_required_tx_fee(tx_size)
        .checked_sub(paid_fee)
        .unwrap_or(0);

    // find a cell to pay tx fee
    if required_fee > 0 {
        // get payment cells
        let cells = cell_collector.query_payment_cells(&lock_hash, required_fee);
        // put cells in tx skeleton
        tx_skeleton
            .inputs_mut()
            .extend(cells.into_iter().map(|cell| {
                CellInput::new_builder()
                    .previous_output(cell.out_point)
                    .build()
            }));
    }
    Ok(())
}
