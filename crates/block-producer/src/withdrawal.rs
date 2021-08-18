use anyhow::{anyhow, Result};
use gw_common::CKB_SUDT_SCRIPT_ARGS;
use gw_config::BlockProducerConfig;
use gw_mem_pool::{custodian::calc_ckb_custodian_min_capacity, withdrawal::Generator};
use gw_rpc_client::RPCClient;
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    offchain::{CellInfo, InputCellInfo, RollupContext, WithdrawalsAmount},
    packed::{
        CellDep, CellInput, CellOutput, CustodianLockArgs, DepositLockArgs, GlobalState, L2Block,
        Script, UnlockWithdrawalViaRevert, UnlockWithdrawalWitness, UnlockWithdrawalWitnessUnion,
        WithdrawalRequest, WitnessArgs,
    },
    prelude::*,
};

use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
struct CkbCustodian {
    capacity: u128,
    balance: u128,
    min_capacity: u64,
}

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
) -> Result<Option<GeneratedWithdrawals>> {
    if block.withdrawals().is_empty() {
        return Ok(None);
    }

    let global_state = GlobalState::from_slice(&input_rollup_cell.data)
        .map_err(|_| anyhow!("parse rollup cell global state"))?;
    let last_finalized_block_number = global_state.last_finalized_block_number().unpack();

    let total_withdrawal_amount = sum(block.withdrawals().into_iter());
    let custodian_cells = rpc_client
        .query_finalized_custodian_cells(
            &total_withdrawal_amount,
            calc_ckb_custodian_min_capacity(rollup_context),
            last_finalized_block_number,
        )
        .await?
        .expect_full("collect custodian cells")?;
    log::debug!("custodian inputs {:?}", custodian_cells);

    let mut generator = Generator::new(rollup_context, (&custodian_cells).into());
    for req in block.withdrawals().into_iter() {
        generator
            .include_and_verify(&req, block)
            .map_err(|err| anyhow!("unexpected withdrawal err {}", err))?
    }
    log::debug!("included withdrawals {}", generator.withdrawals().len());

    let custodian_lock_dep = block_producer_config.custodian_cell_lock_dep.clone();
    let sudt_type_dep = block_producer_config.l1_sudt_type_dep.clone();
    let mut cell_deps = vec![custodian_lock_dep.into()];
    if !total_withdrawal_amount.sudt.is_empty() {
        cell_deps.push(sudt_type_dep.into());
    }

    let custodian_inputs = custodian_cells.cells_info.into_iter().map(|cell| {
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

pub struct RevertedWithdrawals {
    pub deps: Vec<CellDep>,
    pub inputs: Vec<InputCellInfo>,
    pub witness_args: Vec<WitnessArgs>,
    pub outputs: Vec<(CellOutput, Bytes)>,
}

pub fn revert(
    rollup_context: &RollupContext,
    block_producer_config: &BlockProducerConfig,
    withdrawal_cells: Vec<CellInfo>,
) -> Result<Option<RevertedWithdrawals>> {
    if withdrawal_cells.is_empty() {
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
    for (idx, withdrawal) in withdrawal_cells.into_iter().enumerate() {
        let custodian_lock = {
            let deposit_lock_args = DepositLockArgs::new_builder()
                .owner_lock_hash(rollup_context.rollup_script_hash.pack())
                .cancel_timeout((idx as u64 + timestamp).pack())
                .build();

            let custodian_lock_args = CustodianLockArgs::new_builder()
                .deposit_lock_args(deposit_lock_args)
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
    let sudt_type_dep = block_producer_config.l1_sudt_type_dep.clone();
    let mut cell_deps = vec![withdrawal_lock_dep.into()];
    if withdrawal_inputs
        .iter()
        .any(|info| info.cell.output.type_().to_opt().is_some())
    {
        cell_deps.push(sudt_type_dep.into())
    }

    Ok(Some(RevertedWithdrawals {
        deps: cell_deps,
        inputs: withdrawal_inputs,
        outputs: custodian_outputs,
        witness_args: withdrawal_witness,
    }))
}

fn sum<Iter: Iterator<Item = WithdrawalRequest>>(reqs: Iter) -> WithdrawalsAmount {
    reqs.fold(
        WithdrawalsAmount::default(),
        |mut total_amount, withdrawal| {
            total_amount.capacity = total_amount
                .capacity
                .saturating_add(withdrawal.raw().capacity().unpack() as u128);

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
        }
    )
}
