use crate::rpc_client::WithdrawalsAmount;
use crate::rpc_client::{CollectedCustodianCells, RPCClient};
use crate::types::{CellInfo, InputCellInfo};

use anyhow::{anyhow, Result};
use gw_common::CKB_SUDT_SCRIPT_ARGS;
use gw_config::BlockProducerConfig;
use gw_generator::RollupContext;
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{
        CellDep, CellInput, CellOutput, CustodianLockArgs, DepositionLockArgs, GlobalState,
        L2Block, RollupAction, RollupActionUnion, Script, ScriptOpt, Uint128,
        UnlockWithdrawalViaRevert, UnlockWithdrawalWitness, UnlockWithdrawalWitnessUnion,
        WithdrawalLockArgs, WithdrawalRequest, WitnessArgs,
    },
    prelude::*,
};

use std::{
    collections::HashSet,
    time::{SystemTime, UNIX_EPOCH},
};

pub struct GeneratedWithdrawals {
    pub deps: Vec<CellDep>,
    pub inputs: Vec<InputCellInfo>,
    pub outputs: Vec<(CellOutput, Bytes)>,
}

// Note: custodian lock search rollup cell in inputs
pub async fn generate(
    input_rollup_cell: &CellInfo,
    rollup_context: &RollupContext,
    block: &L2Block,
    block_producer_config: &BlockProducerConfig,
    rpc_client: &RPCClient,
) -> Result<GeneratedWithdrawals> {
    let global_state = GlobalState::from_slice(&input_rollup_cell.data)
        .map_err(|_| anyhow!("parse rollup cell global state"))?;
    let last_finalized_block_number = global_state.last_finalized_block_number().unpack();

    let withdrawals = block.withdrawals().into_iter();
    let total_withdrawals_amount = withdrawals.fold(
        WithdrawalsAmount::default(),
        |mut total_amount, withdrawal| {
            total_amount.capacity = total_amount
                .capacity
                .saturating_add(withdrawal.raw().capacity().unpack());

            let sudt_script_hash = withdrawal.raw().sudt_script_hash().unpack();
            let sudt_amount = withdrawal.raw().amount().unpack();
            if sudt_amount != 0 {
                match sudt_script_hash {
                    CKB_SUDT_SCRIPT_ARGS => {
                        let account = withdrawal.raw().account_script_hash();
                        log::warn!("{} withdrawal request non-zero sudt amount but it's type hash ckb, ignore this amount", account);
                    }
                    _ => {
                        let total_sudt_amount = total_amount.sudt.entry(sudt_script_hash).or_insert(0u128);
                        *total_sudt_amount = total_sudt_amount.saturating_add(sudt_amount);
                    }
                }
            }

            total_amount
        },
    );

    let custodian_cells = rpc_client
        .query_finalized_custodian_cells(&total_withdrawals_amount, last_finalized_block_number)
        .await?;
    assert!(custodian_cells.fullfilled_sudt_script.len() == total_withdrawals_amount.sudt.len());

    let build_withdraw_output = |req: WithdrawalRequest| -> Result<(CellOutput, Bytes)> {
        let withdrawal_capacity: u64 = req.raw().capacity().unpack();
        let lock_args: Bytes = {
            let withdrawal_lock_args = WithdrawalLockArgs::new_builder()
                .account_script_hash(req.raw().account_script_hash())
                .withdrawal_block_hash(block.hash().pack())
                .withdrawal_block_number(block.raw().number())
                .sudt_script_hash(req.raw().sudt_script_hash())
                .sell_amount(req.raw().sell_amount())
                .sell_capacity(withdrawal_capacity.pack())
                .owner_lock_hash(req.raw().owner_lock_hash())
                .payment_lock_hash(req.raw().payment_lock_hash())
                .build();

            let rollup_type_hash = rollup_context.rollup_script_hash.as_slice().iter();
            rollup_type_hash
                .chain(withdrawal_lock_args.as_slice().iter())
                .cloned()
                .collect()
        };

        let lock = Script::new_builder()
            .code_hash(rollup_context.rollup_config.withdrawal_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(lock_args.pack())
            .build();

        let sudt_type_hash: [u8; 32] = req.raw().sudt_script_hash().unpack();
        let (type_, data): (ScriptOpt, Bytes) =
            if req.raw().amount().unpack() != 0 && sudt_type_hash != CKB_SUDT_SCRIPT_ARGS {
                let fullfilled_sudt_script = &custodian_cells.fullfilled_sudt_script;

                if !fullfilled_sudt_script.contains_key(&sudt_type_hash) {
                    return Err(anyhow!(
                        "expected sudt type script {} not found",
                        sudt_type_hash.pack()
                    ));
                }

                let sudt_type_script = fullfilled_sudt_script.get(&sudt_type_hash).cloned();
                (sudt_type_script.pack(), req.raw().amount().as_bytes())
            } else {
                (None::<Script>.pack(), Bytes::new())
            };

        let required_capacity = {
            let size = (8 + data.len() + type_.as_slice().len() + lock.as_slice().len()) as u64;
            size * 100000000u64
        };
        if required_capacity > withdrawal_capacity {
            return Err(anyhow!(
                "{} withdrawal capacity {} is smaller than minimal required {}",
                req.raw().account_script_hash(),
                withdrawal_capacity,
                required_capacity
            ));
        }

        let withdrawal_cell = CellOutput::new_builder()
            .capacity(withdrawal_capacity.pack())
            .lock(lock)
            .type_(type_)
            .build();

        Ok((withdrawal_cell, data))
    };

    let change_outputs = generate_change_custodian_outputs(
        rollup_context,
        &custodian_cells,
        &total_withdrawals_amount,
    )?;

    let mut withdrawal_outputs = block
        .withdrawals()
        .into_iter()
        .map(build_withdraw_output)
        .collect::<Result<Vec<_>, _>>()?;
    withdrawal_outputs.extend(change_outputs);

    let custodian_lock_dep = block_producer_config.custodian_cell_lock_dep.clone();
    let custodian_inputs = custodian_cells.cells_info.into_iter().map(|cell| {
        let input = CellInput::new_builder()
            .previous_output(cell.out_point.clone())
            .build();
        InputCellInfo { input, cell }
    });

    let generated_withdrawals = GeneratedWithdrawals {
        deps: vec![custodian_lock_dep.into()],
        inputs: custodian_inputs.collect(),
        outputs: withdrawal_outputs,
    };

    Ok(generated_withdrawals)
}

pub struct RevertedWithdrawals {
    pub deps: Vec<CellDep>,
    pub inputs: Vec<InputCellInfo>,
    pub witness_args: Vec<WitnessArgs>,
    pub outputs: Vec<(CellOutput, Bytes)>,
}

pub async fn revert(
    rollup_action: &RollupAction,
    rollup_context: &RollupContext,
    block_producer_config: &BlockProducerConfig,
    rpc_client: &RPCClient,
) -> Result<Option<RevertedWithdrawals>> {
    let submit_block = match rollup_action.to_enum() {
        RollupActionUnion::RollupSubmitBlock(submit_block) => submit_block,
        _ => return Ok(None),
    };

    if submit_block.reverted_block_hashes().is_empty() {
        return Ok(None);
    }

    let reverted_block_hashes: HashSet<[u8; 32]> = submit_block
        .reverted_block_hashes()
        .into_iter()
        .map(|h| h.unpack())
        .collect();

    let reverted_withdrawal_cells = rpc_client
        .query_withdrawal_cells_by_block_hashes(&reverted_block_hashes)
        .await?;
    if reverted_withdrawal_cells.is_empty() {
        return Ok(None);
    }

    let mut withdrawal_inputs = vec![];
    let mut withdrawal_witness = vec![];
    let mut custodian_outputs = vec![];

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unexpected timestamp")
        .as_millis() as u64;

    // We use timestamp plus idx and rollup_type_hash to create different custodian lock
    // hash for every reverted withdrawal input. Withdrawal lock use custodian lock hash to
    // index corresponding custodian output.
    // NOTE: These locks must also be different from custodian change cells created by
    // withdrawal requests processing.
    let rollup_type_hash = rollup_context.rollup_script_hash.as_slice().iter();
    for (idx, withdrawal) in reverted_withdrawal_cells.into_iter().enumerate() {
        let custodian_lock = {
            let deposition_lock_args = DepositionLockArgs::new_builder()
                .owner_lock_hash(rollup_context.rollup_script_hash.pack())
                .cancel_timeout((idx as u64 + timestamp).pack())
                .build();

            let custodian_lock_args = CustodianLockArgs::new_builder()
                .deposition_lock_args(deposition_lock_args)
                .build();

            let lock_args: Bytes = rollup_type_hash
                .clone()
                .chain(custodian_lock_args.as_slice().iter())
                .cloned()
                .collect();

            Script::new_builder()
                .code_hash(rollup_context.rollup_config.custodian_script_type_hash())
                .hash_type(ScriptHashType::Type.into())
                .args(lock_args.pack())
                .build()
        };

        let custodian_output = {
            let output_builder = withdrawal.output.clone().as_builder();
            output_builder.lock(custodian_lock.clone()).build()
        };

        let withdrawal_input = {
            let input = CellInput::new_builder()
                .previous_output(withdrawal.out_point.clone())
                .build();

            InputCellInfo {
                input,
                cell: withdrawal.clone(),
            }
        };

        let unlock_withdrawal_witness = {
            let unlock_withdrawal_via_revert = UnlockWithdrawalViaRevert::new_builder()
                .custodian_lock_hash(custodian_lock.hash().pack())
                .build();

            UnlockWithdrawalWitness::new_builder()
                .set(UnlockWithdrawalWitnessUnion::UnlockWithdrawalViaRevert(
                    unlock_withdrawal_via_revert,
                ))
                .build()
        };
        let withdrawal_witness_args = WitnessArgs::new_builder()
            .lock(Some(unlock_withdrawal_witness.as_bytes()).pack())
            .build();

        withdrawal_inputs.push(withdrawal_input);
        withdrawal_witness.push(withdrawal_witness_args);
        custodian_outputs.push((custodian_output, withdrawal.data.clone()));
    }

    let withdrawal_lock_dep = block_producer_config.withdrawal_cell_lock_dep.clone();
    let rollup_config_cell_dep = rpc_client.rollup_config_cell_dep.clone();

    Ok(Some(RevertedWithdrawals {
        deps: vec![rollup_config_cell_dep, withdrawal_lock_dep.into()],
        inputs: withdrawal_inputs,
        outputs: custodian_outputs,
        witness_args: withdrawal_witness,
    }))
}

fn generate_change_custodian_outputs(
    rollup_context: &RollupContext,
    collected: &CollectedCustodianCells,
    withdrawals_amount: &WithdrawalsAmount,
) -> Result<Vec<(CellOutput, Bytes)>> {
    let mut used_capacity = 0u64;
    let mut change_outputs = Vec::with_capacity(withdrawals_amount.sudt.len() + 1); // plus one for pure ckb change custodian cell

    let custodian_lock_script = {
        let rollup_type_hash = rollup_context.rollup_script_hash.as_slice().iter();
        let custodian_lock_args = CustodianLockArgs::default();

        let args: Bytes = rollup_type_hash
            .chain(custodian_lock_args.as_slice().iter())
            .cloned()
            .collect();

        Script::new_builder()
            .code_hash(rollup_context.rollup_config.custodian_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(args.pack())
            .build()
    };

    // Generate sudt change custodian outputs
    for (sudt_type_hash, withdrawal_amount) in withdrawals_amount.sudt.iter() {
        let collected_amount = match collected.sudt.get(sudt_type_hash) {
            Some(collected_amount) => collected_amount,
            None => {
                return Err(anyhow!(
                    "expected collected {} sudt amount not found",
                    sudt_type_hash.pack()
                ))
            }
        };
        let sudt_type_script = match collected.fullfilled_sudt_script.get(sudt_type_hash) {
            Some(type_script) => type_script,
            None => {
                return Err(anyhow!(
                    "expected withdrawal sudt {} type script not found",
                    sudt_type_hash.pack()
                ))
            }
        };

        // Withdrawal all collected sudt amount
        if collected_amount == withdrawal_amount {
            log::debug!(
                "collected {:?} amount equal to withdrawal amount",
                sudt_type_hash
            );
            continue;
        }

        let change_amount = collected_amount.saturating_sub(*withdrawal_amount);
        let data: Uint128 = change_amount.pack();

        let change_capacity = (8
            + data.as_slice().len()
            + sudt_type_script.as_slice().len()
            + custodian_lock_script.as_slice().len()) as u64
            * 100000000u64;

        used_capacity = used_capacity.saturating_add(change_capacity);
        if collected.capacity < used_capacity {
            return Err(anyhow!(
                "no enough capacity left to generate sudt change custodian cell"
            ));
        }

        let change_custodian_output = CellOutput::new_builder()
            .capacity(change_capacity.pack())
            .type_(Some(sudt_type_script.to_owned()).pack())
            .lock(custodian_lock_script.clone())
            .build();

        change_outputs.push((change_custodian_output, data.as_bytes()));
    }

    if collected.capacity == used_capacity {
        return Ok(change_outputs);
    }

    let min_ckb_change_capacity =
        (8u64 + custodian_lock_script.as_slice().len() as u64) * 100000000u64;
    if collected.capacity < used_capacity.saturating_add(min_ckb_change_capacity) {
        return Err(anyhow!(
            "no enouth capacity left to generate pure ckb change custodian cell"
        ));
    }

    let ckb_change_capacity = collected.capacity.saturating_sub(used_capacity);
    let ckb_change_custodian_output = CellOutput::new_builder()
        .capacity(ckb_change_capacity.pack())
        .lock(custodian_lock_script)
        .build();

    change_outputs.push((ckb_change_custodian_output, Bytes::new()));
    Ok(change_outputs)
}
