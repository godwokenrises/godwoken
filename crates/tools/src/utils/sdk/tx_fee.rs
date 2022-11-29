use anyhow::anyhow;
use thiserror::Error;

use ckb_types::{
    core::{error::OutPointError, Capacity, CapacityError, TransactionView},
    prelude::*,
};

use super::traits::{TransactionDependencyError, TransactionDependencyProvider};
use super::util::calculate_dao_maximum_withdraw4;
use super::{constants::DAO_TYPE_HASH, traits::HeaderDepResolver};

#[derive(Error, Debug)]
pub enum TransactionFeeError {
    #[error("transaction dependency provider error: `{0}`")]
    TxDep(#[from] TransactionDependencyError),

    #[error("header dependency provider error: `{0}`")]
    HeaderDep(#[from] anyhow::Error),

    #[error("out point error: `{0}`")]
    OutPoint(#[from] OutPointError),

    #[error("unexpected dao withdraw cell in inputs")]
    UnexpectedDaoWithdrawInput,

    #[error("capacity error: `{0}`")]
    CapacityError(#[from] CapacityError),

    #[error("capacity sub overflow, delta: `{0}`")]
    CapacityOverflow(u64),
}

/// Calculate the actual transaction fee of the transaction, include dao
/// withdraw capacity.
#[allow(clippy::unnecessary_lazy_evaluations)]
pub fn tx_fee(
    tx: TransactionView,
    tx_dep_provider: &dyn TransactionDependencyProvider,
    header_dep_resolver: &dyn HeaderDepResolver,
) -> Result<u64, TransactionFeeError> {
    let mut input_total: u64 = 0;
    for input in tx.inputs() {
        let mut is_withdraw = false;
        let since: u64 = input.since().unpack();
        let cell = tx_dep_provider.get_cell(&input.previous_output())?;
        if since != 0 {
            if let Some(type_script) = cell.type_().to_opt() {
                if type_script.code_hash().as_slice() == DAO_TYPE_HASH.as_bytes() {
                    is_withdraw = true;
                }
            }
        }
        let capacity: u64 = if is_withdraw {
            let tx_hash = input.previous_output().tx_hash();
            let prepare_header = header_dep_resolver
                .resolve_by_tx(&tx_hash)
                .map_err(TransactionFeeError::HeaderDep)?
                .ok_or_else(|| {
                    TransactionFeeError::HeaderDep(anyhow!(
                        "resolve prepare header by transaction hash failed: {}",
                        tx_hash
                    ))
                })?;
            let data = tx_dep_provider.get_cell_data(&input.previous_output())?;
            assert_eq!(data.len(), 8);
            let deposit_number = {
                let mut number_bytes = [0u8; 8];
                number_bytes.copy_from_slice(data.as_ref());
                u64::from_le_bytes(number_bytes)
            };
            let deposit_header = header_dep_resolver
                .resolve_by_number(deposit_number)
                .map_err(TransactionFeeError::HeaderDep)?
                .ok_or_else(|| {
                    TransactionFeeError::HeaderDep(anyhow!(
                        "resolve deposit header by block number failed: {}",
                        deposit_number
                    ))
                })?;
            let occupied_capacity = cell
                .occupied_capacity(Capacity::bytes(data.len()).unwrap())
                .unwrap();
            calculate_dao_maximum_withdraw4(
                &deposit_header,
                &prepare_header,
                &cell,
                occupied_capacity.as_u64(),
            )
        } else {
            cell.capacity().unpack()
        };
        input_total += capacity;
    }
    let output_total = tx.outputs_capacity()?.as_u64();
    #[allow(clippy::unnecessary_lazy_evaluations)]
    input_total
        .checked_sub(output_total)
        .ok_or_else(|| TransactionFeeError::CapacityOverflow(output_total - input_total))
}
