#![allow(clippy::mutable_key_type)]

use crate::transaction_skeleton::TransactionSkeleton;
use anyhow::{anyhow, Result};
use gw_config::FeeConfig;
use gw_generator::backend_manage::BackendType;
use gw_rpc_client::indexer_client::CKBIndexerClient;
use gw_types::{
    offchain::InputCellInfo,
    packed::{
        CellInput, CellOutput, MetaContractArgs, MetaContractArgsUnion, SUDTArgs, SUDTArgsUnion,
        Script,
    },
    prelude::*,
};
use std::convert::TryInto;

/// check if the fee or fee_rate/gasPrice of the L2Transaction is enough
/// - check the fee of MetaContract::CreateAccount
/// - check the fee of SUDTTransfer
/// - check the gasPrice of Polyjuice L2TX
///     gasPrice: Value of the gas for a transaction
///
/// @return if the fee is too low for acceptance, return an anyhow ad-hoc Error
pub fn check_l2tx_fee(
    fee_config: &FeeConfig,
    raw_l2tx: &gw_types::packed::RawL2Transaction,
    backend_type: BackendType,
) -> Result<()> {
    let raw_l2tx_args = raw_l2tx.args().raw_data();
    match backend_type {
        BackendType::Meta => {
            let meta_contract_args = MetaContractArgs::from_slice(raw_l2tx_args.as_ref())?;
            let fee_struct = match meta_contract_args.to_enum() {
                MetaContractArgsUnion::CreateAccount(args) => args.fee(),
            };
            if !fee_config.is_supported_sudt(fee_struct.sudt_id().unpack()) {
                return Err(anyhow!("Only support using CKB to pay fee."));
            }
            let meta_contract_base_fee = fee_config.meta_contract_base_fee();
            if fee_struct.amount().unpack() < meta_contract_base_fee {
                let err_msg = format!("The fee is too low for acceptance, should more than meta_contract_base_fee({} shannons).",
                meta_contract_base_fee);
                log::warn!("[check_l2tx_fee] {}", err_msg);
                return Err(anyhow!(err_msg));
            }
            Ok(())
        }
        BackendType::Sudt => {
            let sudt_id = raw_l2tx.to_id();
            if !fee_config.is_supported_sudt(sudt_id.unpack()) {
                return Err(anyhow!("Only support using CKB to pay fee. Please use SudtERC20Proxy to transfer sUDT instead."));
            }
            let sudt_args = SUDTArgs::from_slice(raw_l2tx_args.as_ref())?;
            let fee_amount = match sudt_args.to_enum() {
                SUDTArgsUnion::SUDTQuery(_) => 0u128,
                SUDTArgsUnion::SUDTTransfer(args) => args.fee().unpack(),
            };
            let sudt_transfer_base_fee = fee_config.sudt_transfer_base_fee();
            if fee_amount < sudt_transfer_base_fee {
                let err_msg = format!("The fee is too low for acceptance, should more than sudt_transfer_base_fee({} shannons).",
                sudt_transfer_base_fee);
                log::warn!("[check_l2tx_fee] {}", err_msg);
                return Err(anyhow!(err_msg));
            }
            Ok(())
        }
        BackendType::Polyjuice => {
            // verify the args of a polyjuice L2TX
            // https://github.com/nervosnetwork/godwoken-polyjuice/blob/aee95c0/README.md#polyjuice-arguments
            if raw_l2tx_args.len() < (8 + 8 + 16 + 16 + 4) {
                return Err(anyhow!("invalid PolyjuiceArgs"));
            }
            // Note: Polyjuice use CKB_SUDT to pay fee by default
            let poly_args = raw_l2tx_args.as_ref();
            let gas_price = u128::from_le_bytes(poly_args[16..32].try_into()?);
            let min_gas_price = fee_config.polyjuice_base_gas_price();
            if gas_price < min_gas_price {
                log::warn!(
                    "Gas Price too low for acceptance, should more than polyjuice_base_gas_price({} shannons).",
                    min_gas_price);
                return Err(anyhow!("Gas Price too low for acceptance, should more than polyjuice_base_gas_price({} shannons).",
                min_gas_price));
            }
            Ok(())
        }
    }
}

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
    // NOTE: Poa will insert a owner cell to inputs if there isn't one in ```fill_poa()```,
    // so most of time, paid_fee should already cover tx_fee. The first thing we need to do
    // is try to generate a change output cell.
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
