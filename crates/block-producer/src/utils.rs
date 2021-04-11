use crate::types::InputCellInfo;
use crate::{rpc_client::RPCClient, transaction_skeleton::TransactionSkeleton};
use anyhow::Result;
use async_jsonrpc_client::Output;
use gw_types::{
    packed::{CellInput, Script},
    prelude::*,
};
use serde::de::DeserializeOwned;
use serde_json::from_value;

// convert json output to result
pub fn to_result<T: DeserializeOwned>(output: Output) -> anyhow::Result<T> {
    match output {
        Output::Success(success) => Ok(from_value(success.result)?),
        Output::Failure(failure) => Err(anyhow::anyhow!("JSONRPC error: {}", failure.error)),
    }
}

/// Calculate tx fee
fn calculate_required_tx_fee(tx_size: usize) -> u64 {
    // tx_size * KB / MIN_FEE_RATE
    tx_size as u64
}

/// Add fee cell to tx skeleton
pub async fn fill_tx_fee(
    tx_skeleton: &mut TransactionSkeleton,
    rpc_client: &RPCClient,
    lock_script: Script,
) -> Result<()> {
    let tx_size = tx_skeleton.tx_in_block_size()?;
    let paid_fee: u64 = tx_skeleton.calculate_fee()?;
    // calculate required fee
    let required_fee = calculate_required_tx_fee(tx_size).saturating_sub(paid_fee);

    // find a cell to pay tx fee
    if required_fee > 0 {
        // get payment cells
        let cells = rpc_client
            .query_payment_cells(lock_script, required_fee)
            .await?;
        assert!(cells.len() > 0, "need cells to pay fee");
        // put cells in tx skeleton
        tx_skeleton
            .inputs_mut()
            .extend(cells.into_iter().map(|cell| {
                let input = CellInput::new_builder()
                    .previous_output(cell.out_point.clone())
                    .build();
                InputCellInfo { input, cell }
            }));
    }

    {
        let paid_fee: u64 = tx_skeleton.calculate_fee()?;
        // calculate required fee
        let required_fee = calculate_required_tx_fee(tx_size).saturating_sub(paid_fee);
        assert_eq!(required_fee, 0, "should have enough tx fee");
    }
    Ok(())
}
