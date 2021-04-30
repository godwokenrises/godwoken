use crate::indexer_types::{Cell, Order, Pagination, ScriptType, SearchKey};
use crate::rpc_client::{to_result, RPCClient, DEFAULT_QUERY_LIMIT};
use crate::types::{CellInfo, InputCellInfo};

use anyhow::{anyhow, Result};
use async_jsonrpc_client::{Params as ClientParams, Transport};
use gw_common::CKB_SUDT_SCRIPT_ARGS;
use gw_config::BlockProducerConfig;
use gw_generator::RollupContext;
use gw_jsonrpc_types::ckb_jsonrpc_types::Uint32;
use gw_types::{
    bytes::Bytes,
    core::{DepType, ScriptHashType},
    packed::{
        CellDep, CellInput, CellOutput, CustodianLockArgs, CustodianLockArgsReader,
        DepositionLockArgs, GlobalState, L2Block, OutPoint, RollupAction, RollupActionUnion,
        Script, ScriptOpt, Uint128, UnlockWithdrawalViaRevert, UnlockWithdrawalWitness,
        UnlockWithdrawalWitnessUnion, WithdrawalLockArgs, WithdrawalLockArgsReader,
        WithdrawalRequest, WitnessArgs,
    },
    prelude::*,
};
use serde_json::json;

use std::collections::{HashMap, HashSet};

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
                        eprintln!("{} withdrawal request non-zero sudt amount but it's type hash ckb, ignore this amount", account);
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

    let custodian_cells = query_finalized_custodian_cells(
        rpc_client,
        &total_withdrawals_amount,
        last_finalized_block_number,
    )
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

    let rollup_config_cell_dep = match query_rollup_config_cell(rpc_client).await? {
        Some(rollup_config_cell) => CellDep::new_builder()
            .out_point(rollup_config_cell.out_point.to_owned())
            .dep_type(DepType::Code.into())
            .build(),
        None => {
            return Err(anyhow::anyhow!("rollup config cell not found"));
        }
    };

    let reverted_withdrawal_cells =
        query_reverted_withdrawal_cells(rpc_client, &reverted_block_hashes).await?;
    if reverted_withdrawal_cells.is_empty() {
        return Ok(None);
    }

    let mut withdrawal_inputs = vec![];
    let mut withdrawal_witness = vec![];
    let mut custodian_outputs = vec![];

    // NOTE: We use idx to create different custodian lock hash for every reverted withdrawal
    // input. Withdrawal lock use custodian lock hash to index corresponding custodian output.
    let rollup_type_hash = rollup_context.rollup_script_hash.as_slice().iter();
    for (idx, withdrawal) in reverted_withdrawal_cells.into_iter().enumerate() {
        let custodian_lock = {
            let deposition_lock_args = DepositionLockArgs::new_builder()
                .cancel_timeout((idx as u64).pack())
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

    Ok(Some(RevertedWithdrawals {
        deps: vec![rollup_config_cell_dep.into(), withdrawal_lock_dep.into()],
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
            println!(
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

#[derive(Debug)]
struct WithdrawalsAmount {
    pub capacity: u64,
    pub sudt: HashMap<[u8; 32], u128>,
}

impl Default for WithdrawalsAmount {
    fn default() -> Self {
        WithdrawalsAmount {
            capacity: 0,
            sudt: Default::default(),
        }
    }
}

#[derive(Debug)]
struct CollectedCustodianCells {
    pub cells_info: Vec<CellInfo>,
    pub capacity: u64,
    pub sudt: HashMap<[u8; 32], u128>,
    pub fullfilled_sudt_script: HashMap<[u8; 32], Script>,
}

impl Default for CollectedCustodianCells {
    fn default() -> Self {
        CollectedCustodianCells {
            cells_info: Default::default(),
            capacity: 0,
            sudt: Default::default(),
            fullfilled_sudt_script: Default::default(),
        }
    }
}

async fn query_finalized_custodian_cells(
    rpc_client: &RPCClient,
    withdrawals_amount: &WithdrawalsAmount,
    last_finalized_block_number: u64,
) -> Result<CollectedCustodianCells> {
    let rollup_context = &rpc_client.rollup_context;

    let custodian_lock = Script::new_builder()
        .code_hash(rollup_context.rollup_config.custodian_script_type_hash())
        .hash_type(ScriptHashType::Type.into())
        .args(rollup_context.rollup_script_hash.as_slice().pack())
        .build();

    let search_key = SearchKey {
        script: ckb_types::packed::Script::new_unchecked(custodian_lock.as_bytes()).into(),
        script_type: ScriptType::Lock,
        filter: None,
    };
    let order = Order::Desc;
    let limit = Uint32::from(DEFAULT_QUERY_LIMIT as u32);

    let mut collected = CollectedCustodianCells::default();
    let mut cursor = None;

    while collected.capacity < withdrawals_amount.capacity
        || collected.fullfilled_sudt_script.len() < withdrawals_amount.sudt.len()
    {
        let cells: Pagination<Cell> = to_result(
            rpc_client
                .indexer_client
                .request(
                    "get_cells",
                    Some(ClientParams::Array(vec![
                        json!(search_key),
                        json!(order),
                        json!(limit),
                        json!(cursor),
                    ])),
                )
                .await?,
        )?;

        if cells.last_cursor.is_empty() {
            return Err(anyhow!("no finalized custodian cell"));
        }
        cursor = Some(cells.last_cursor);

        for cell in cells.objects.into_iter() {
            let args = cell.output.lock.args.clone().into_bytes();
            let custodian_lock_args = match CustodianLockArgsReader::verify(&args[32..], false) {
                Ok(()) => CustodianLockArgs::new_unchecked(args.slice(32..)),
                Err(_) => continue,
            };

            if custodian_lock_args.deposition_block_number().unpack() > last_finalized_block_number
            {
                continue;
            }

            // Collect sudt
            if let Some(json_script) = cell.output.type_.clone() {
                let sudt_type_script = {
                    let script = ckb_types::packed::Script::from(json_script);
                    Script::new_unchecked(script.as_bytes())
                };

                let sudt_type_hash = sudt_type_script.hash();
                if sudt_type_hash != CKB_SUDT_SCRIPT_ARGS {
                    // Already collected enough sudt amount
                    let fullfilled_sudt_script = &mut collected.fullfilled_sudt_script;
                    if fullfilled_sudt_script.contains_key(&sudt_type_hash) {
                        continue;
                    }

                    // Not targed withdrawal sudt
                    let withdrawal_amount = match withdrawals_amount.sudt.get(&sudt_type_hash) {
                        Some(amount) => amount,
                        None => continue,
                    };

                    let sudt_amount = match parse_sudt_amount(&cell) {
                        Ok(amount) => amount,
                        Err(_) => {
                            eprintln!("invalid sudt amount, out_point: {:?}", cell.out_point);
                            continue;
                        }
                    };

                    let collected_amount = collected.sudt.entry(sudt_type_hash).or_insert(0);
                    *collected_amount = collected_amount.saturating_add(sudt_amount);

                    if *collected_amount >= *withdrawal_amount {
                        fullfilled_sudt_script.insert(sudt_type_hash.to_owned(), sudt_type_script);
                    }
                }
            }

            // Collect capacity
            let out_point = {
                let out_point: ckb_types::packed::OutPoint = cell.out_point.into();
                OutPoint::new_unchecked(out_point.as_bytes())
            };

            let output = {
                let output: ckb_types::packed::CellOutput = cell.output.into();
                CellOutput::new_unchecked(output.as_bytes())
            };

            collected.capacity = collected
                .capacity
                .saturating_add(output.capacity().unpack());

            let info = CellInfo {
                out_point,
                output,
                data: cell.output_data.into_bytes(),
            };

            collected.cells_info.push(info);
        }
    }

    Ok(collected)
}

async fn query_rollup_config_cell(rpc_client: &RPCClient) -> Result<Option<CellInfo>> {
    let search_key = SearchKey {
        script: rpc_client.rollup_config_type_script.clone().into(),
        script_type: ScriptType::Type,
        filter: None,
    };
    let order = Order::Desc;
    let limit = Uint32::from(1);

    let mut cells: Pagination<Cell> = to_result(
        rpc_client
            .indexer_client
            .request(
                "get_cells",
                Some(ClientParams::Array(vec![
                    json!(search_key),
                    json!(order),
                    json!(limit),
                ])),
            )
            .await?,
    )?;
    if let Some(cell) = cells.objects.pop() {
        let out_point = {
            let out_point: ckb_types::packed::OutPoint = cell.out_point.into();
            OutPoint::new_unchecked(out_point.as_bytes())
        };
        let output = {
            let output: ckb_types::packed::CellOutput = cell.output.into();
            CellOutput::new_unchecked(output.as_bytes())
        };
        let data = cell.output_data.into_bytes();
        let cell_info = CellInfo {
            out_point,
            output,
            data,
        };
        return Ok(Some(cell_info));
    }
    Ok(None)
}

async fn query_reverted_withdrawal_cells(
    rpc_client: &RPCClient,
    reverted_block_hashes: &HashSet<[u8; 32]>,
) -> Result<Vec<CellInfo>> {
    let rollup_context = &rpc_client.rollup_context;

    let withdrawal_lock = Script::new_builder()
        .code_hash(rollup_context.rollup_config.withdrawal_script_type_hash())
        .hash_type(ScriptHashType::Type.into())
        .args(rollup_context.rollup_script_hash.as_slice().pack())
        .build();

    let search_key = SearchKey {
        script: ckb_types::packed::Script::new_unchecked(withdrawal_lock.as_bytes()).into(),
        script_type: ScriptType::Lock,
        filter: None,
    };
    let order = Order::Desc;
    let limit = Uint32::from(DEFAULT_QUERY_LIMIT as u32);

    let mut collected = vec![];
    let mut cursor = None;

    while collected.is_empty() {
        let cells: Pagination<Cell> = to_result(
            rpc_client
                .indexer_client
                .request(
                    "get_cells",
                    Some(ClientParams::Array(vec![
                        json!(search_key),
                        json!(order),
                        json!(limit),
                        json!(cursor),
                    ])),
                )
                .await?,
        )?;

        if cells.last_cursor.is_empty() {
            return Ok(vec![]);
        }
        cursor = Some(cells.last_cursor);

        for cell in cells.objects.into_iter() {
            let args = cell.output.lock.args.clone().into_bytes();
            let withdrawal_lock_args = match WithdrawalLockArgsReader::verify(&args[32..], false) {
                Ok(()) => WithdrawalLockArgs::new_unchecked(args.slice(32..)),
                Err(_) => continue,
            };

            let withdrawal_block_hash: [u8; 32] =
                withdrawal_lock_args.withdrawal_block_hash().unpack();
            if !reverted_block_hashes.contains(&withdrawal_block_hash) {
                continue;
            }

            let out_point = {
                let out_point: ckb_types::packed::OutPoint = cell.out_point.into();
                OutPoint::new_unchecked(out_point.as_bytes())
            };

            let output = {
                let output: ckb_types::packed::CellOutput = cell.output.into();
                CellOutput::new_unchecked(output.as_bytes())
            };

            let info = CellInfo {
                out_point,
                output,
                data: cell.output_data.into_bytes(),
            };

            collected.push(info);
        }
    }

    Ok(collected)
}

fn parse_sudt_amount(cell: &Cell) -> Result<u128> {
    if cell.output.type_.is_none() {
        return Err(anyhow!("no a sudt cell"));
    }

    gw_types::packed::Uint128::from_slice(&cell.output_data.as_bytes())
        .map(|a| a.unpack())
        .map_err(|e| anyhow!("invalid sudt amount {}", e))
}
