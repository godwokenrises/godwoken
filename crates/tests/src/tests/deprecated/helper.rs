#![allow(clippy::mutable_key_type)]

use anyhow::{anyhow, Result};
use gw_common::H256;
use gw_config::ContractsCellDep;
use gw_mem_pool::{custodian::sum_withdrawals, withdrawal::Generator};
use gw_types::{
    bytes::Bytes,
    offchain::{CollectedCustodianCells, InputCellInfo, RollupContext},
    packed::{CellDep, CellInput, CellOutput, L2Block, WithdrawalRequestExtra},
    prelude::*,
};

use std::collections::HashMap;

pub struct GeneratedWithdrawals {
    pub deps: Vec<CellDep>,
    pub inputs: Vec<InputCellInfo>,
    pub outputs: Vec<(CellOutput, Bytes)>,
}

// Note: custodian lock search rollup cell in inputs
// TODO: remove after unlock all withdrawal cell on chain
pub fn generate(
    rollup_context: &RollupContext,
    finalized_custodians: CollectedCustodianCells,
    block: &L2Block,
    contracts_dep: &ContractsCellDep,
    withdrawal_extras: &HashMap<H256, WithdrawalRequestExtra>,
) -> Result<Option<GeneratedWithdrawals>> {
    if block.withdrawals().is_empty() && finalized_custodians.cells_info.len() <= 1 {
        return Ok(None);
    }
    // println!("custodian inputs {:?}", finalized_custodians);

    let total_withdrawal_amount = sum_withdrawals(block.withdrawals().into_iter());
    let mut generator = Generator::new(rollup_context, (&finalized_custodians).into());
    for req in block.withdrawals().into_iter() {
        let req_extra = match withdrawal_extras.get(&req.hash().into()) {
            Some(req_extra) => req_extra.to_owned(),
            None => WithdrawalRequestExtra::new_builder().request(req).build(),
        };
        generator
            .include_and_verify(&req_extra, block)
            .map_err(|err| anyhow!("unexpected withdrawal err {}", err))?
    }
    // println!("included withdrawals {}", generator.withdrawals().len());

    let custodian_lock_dep = contracts_dep.custodian_cell_lock.clone();
    let sudt_type_dep = contracts_dep.l1_sudt_type.clone();
    let mut cell_deps = vec![custodian_lock_dep.into()];
    if !total_withdrawal_amount.sudt.is_empty() || !finalized_custodians.sudt.is_empty() {
        cell_deps.push(sudt_type_dep.into());
    }

    let custodian_inputs = finalized_custodians.cells_info.into_iter().map(|cell| {
        let input = CellInput::new_builder()
            .previous_output(cell.out_point.clone())
            .build();
        InputCellInfo { input, cell }
    });

    let generated_withdrawals = GeneratedWithdrawals {
        deps: cell_deps,
        inputs: custodian_inputs.collect(),
        outputs: generator.finish(),
    };

    Ok(Some(generated_withdrawals))
}
