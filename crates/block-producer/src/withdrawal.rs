use crate::rpc_client::{CollectedCustodianCells, RPCClient, WithdrawalsAmount};
use crate::types::InputCellInfo;

use anyhow::{anyhow, Result};
use gw_common::CKB_SUDT_SCRIPT_ARGS;
use gw_config::BlockProducerConfig;
use gw_generator::RollupContext;
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{
        CellDep, CellInput, CellOutput, CustodianLockArgs, DepositionLockArgs, L2Block,
        RollupAction, RollupActionUnion, Script, UnlockWithdrawalViaRevert,
        UnlockWithdrawalWitness, UnlockWithdrawalWitnessUnion, WithdrawalLockArgs,
        WithdrawalRequest, WitnessArgs,
    },
    prelude::*,
};
use parking_lot::Mutex;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug)]
pub struct AvailableCustodians {
    pub capacity: u64,
    pub sudt: HashMap<[u8; 32], (u128, Script)>,
}

impl Default for AvailableCustodians {
    fn default() -> Self {
        AvailableCustodians {
            capacity: 0,
            sudt: Default::default(),
        }
    }
}

impl<'a> From<&'a CollectedCustodianCells> for AvailableCustodians {
    fn from(collected: &'a CollectedCustodianCells) -> Self {
        AvailableCustodians {
            capacity: collected.capacity,
            sudt: collected.sudt.clone(),
        }
    }
}

pub struct Generator<'a> {
    rollup_context: &'a RollupContext,
    ckb_custodian: (u64, u64, u64), // (capacity, balance, min_capacity)
    sudt_custodians: HashMap<[u8; 32], (u64, u128, Script)>, // (capacity, balance, script)
    withdrawals: Vec<(CellOutput, Bytes)>,
}

impl<'a> Generator<'a> {
    pub fn new(
        rollup_context: &'a RollupContext,
        available_custodians: AvailableCustodians,
    ) -> Self {
        let mut total_sudt_capacity = 0u64;
        let mut sudt_custodians = HashMap::new();

        for (sudt_type_hash, (balance, type_script)) in available_custodians.sudt.into_iter() {
            let (change, _data) =
                generate_finalized_custodian(rollup_context, balance, type_script.clone());
            let change_capacity: u64 = change.capacity().unpack();
            total_sudt_capacity = total_sudt_capacity.saturating_add(change_capacity);
            sudt_custodians.insert(sudt_type_hash, (change_capacity, balance, type_script));
        }

        let ckb_custodian_min_capacity = {
            let lock = build_finalized_custodian_lock(rollup_context);
            (8 + lock.as_slice().len() as u64) * 100000000
        };

        let ckb_custodian_capacity = available_custodians
            .capacity
            .saturating_sub(total_sudt_capacity);
        let ckb_balance = ckb_custodian_capacity.saturating_sub(ckb_custodian_min_capacity);
        let ckb_custodian = (
            ckb_custodian_capacity,
            ckb_balance,
            ckb_custodian_min_capacity,
        );

        Generator {
            rollup_context,
            ckb_custodian,
            sudt_custodians,
            withdrawals: Default::default(),
        }
    }

    pub fn include_and_verify(&mut self, req: &WithdrawalRequest, block: &L2Block) -> Result<()> {
        // Verify finalized custodian exists
        let req_sudt: u128 = req.raw().amount().unpack();
        let sudt_type_hash: [u8; 32] = req.raw().sudt_script_hash().unpack();
        if 0 != req_sudt && !self.sudt_custodians.contains_key(&sudt_type_hash) {
            return Err(anyhow!("no finalized sudt custodian for {}", req));
        }

        // Verify minimal capacity
        let req_ckb: u64 = req.raw().capacity().unpack();
        let sudt_script = {
            let sudt_custodain = self.sudt_custodians.get(&sudt_type_hash);
            sudt_custodain.map(|(_, _, script)| script.to_owned())
        };
        let output = generate_withdrawal_output(req, self.rollup_context, block, sudt_script)
            .map_err(|min_capacity| anyhow!("{} minimal capacity for {}", min_capacity, req))?;

        // Verify remaind sudt
        if 0 != req_sudt {
            let (sudt_custodian_capacity, sudt_balance, _) =
                match self.sudt_custodians.get_mut(&sudt_type_hash) {
                    Some(custodian) => custodian,
                    None => return Err(anyhow!("no finalized sudt custodian for {}", req)),
                };

            match sudt_balance.checked_sub(req_sudt) {
                Some(remaind) => *sudt_balance = remaind,
                None => return Err(anyhow!("no enough custodian sudt for {}", req)),
            }

            // Consume all remaind sudt, give sudt custodian capacity back to ckb custodian
            if 0 == *sudt_balance {
                let (ckb_custodian_capacity, ckb_balance, ckb_custodian_min_capacity) =
                    &mut self.ckb_custodian;

                // If ckb custodian is already consumed
                if 0 == *ckb_custodian_capacity {
                    *ckb_custodian_capacity = *sudt_custodian_capacity;
                    *ckb_balance = *sudt_custodian_capacity - *ckb_custodian_min_capacity;
                } else {
                    *ckb_custodian_capacity += *sudt_custodian_capacity;
                    *ckb_balance += *sudt_custodian_capacity;
                }
                *sudt_custodian_capacity = 0;
            }
        }

        // Verify remaind ckb (capacity, available_amount, min_capacity)
        let (ckb_custodian_capacity, ckb_balance, _) = &mut self.ckb_custodian;
        match ckb_balance.checked_sub(req_ckb) {
            Some(remaind) => {
                *ckb_custodian_capacity -= req_ckb;
                *ckb_balance = remaind;
            }
            // Consume all remaind ckb
            None if req_ckb == *ckb_custodian_capacity => {
                *ckb_custodian_capacity = 0;
                *ckb_balance = 0;
            }
            None => return Err(anyhow!("no enough custodian capacity for {}", req)),
        }

        self.withdrawals.push(output);
        Ok(())
    }

    pub fn finish(self) -> Vec<(CellOutput, Bytes)> {
        let mut outputs = self.withdrawals;
        let custodian_lock = build_finalized_custodian_lock(self.rollup_context);

        // Generate sudt custodian changes
        let sudt_changes = {
            let custodians = self.sudt_custodians.into_iter();
            custodians.filter(|(_, (capacity, balance, _))| 0 != *capacity && 0 != *balance)
        };
        for (capacity, balance, script) in sudt_changes.map(|(_, c)| c) {
            let output = CellOutput::new_builder()
                .capacity(capacity.pack())
                .type_(Some(script).pack())
                .lock(custodian_lock.clone())
                .build();

            outputs.push((output, balance.pack().as_bytes()));
        }

        // Generate ckb custodian change
        let (ckb_custodian_capacity, ..) = self.ckb_custodian;
        if 0 != ckb_custodian_capacity {
            let output = CellOutput::new_builder()
                .capacity(ckb_custodian_capacity.pack())
                .lock(custodian_lock)
                .build();

            outputs.push((output, Bytes::new()));
        }

        outputs
    }
}

pub struct GeneratedWithdrawals {
    pub deps: Vec<CellDep>,
    pub inputs: Vec<InputCellInfo>,
    pub outputs: Vec<(CellOutput, Bytes)>,
}

// Note: custodian lock search rollup cell in inputs
pub fn generate(
    rollup_context: &RollupContext,
    block: &L2Block,
    block_producer_config: &BlockProducerConfig,
    custodian_cells: CollectedCustodianCells,
) -> Result<GeneratedWithdrawals> {
    let mut generator = Generator::new(rollup_context, (&custodian_cells).into());

    for req in block.withdrawals().into_iter() {
        generator.include_and_verify(&req, block)?;
    }

    let custodian_lock_dep = block_producer_config.custodian_cell_lock_dep.clone();
    let custodian_inputs = custodian_cells.cells_info.into_iter().map(|cell| {
        let input = CellInput::new_builder()
            .previous_output(cell.out_point.clone())
            .build();
        InputCellInfo { input, cell }
    });

    let withdrawal_outputs = generator.finish();
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
    Ok(Some(RevertedWithdrawals {
        deps: vec![withdrawal_lock_dep.into()],
        inputs: withdrawal_inputs,
        outputs: custodian_outputs,
        witness_args: withdrawal_witness,
    }))
}

pub fn sum<'a, Iter: Iterator<Item = &'a WithdrawalRequest>>(reqs: Iter) -> WithdrawalsAmount {
    reqs.fold(
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
        }
    )
}

pub fn minimal_capacity_verifier(
    rollup_context: RollupContext,
    rpc_client: RPCClient,
) -> Box<dyn Fn(&WithdrawalRequest) -> Result<()> + Send> {
    let sudt_scripts = Arc::new(Mutex::new(HashMap::new()));
    let verifier = move |req: &WithdrawalRequest| -> Result<()> {
        let sudt_script_hash: [u8; 32] = req.raw().sudt_script_hash().unpack();

        // Fetch sudt script
        {
            let has_script = { sudt_scripts.lock().contains_key(&sudt_script_hash) };

            if 0 != req.raw().amount().unpack()
                && sudt_script_hash != CKB_SUDT_SCRIPT_ARGS
                && !has_script
            {
                let sudt_script = match smol::block_on(async {
                    rpc_client
                        .query_custodian_type_script(sudt_script_hash)
                        .await
                })? {
                    Some(script) => script,
                    None => return Err(anyhow!("sudt script not found")),
                };

                sudt_scripts.lock().insert(sudt_script_hash, sudt_script);
            }
        }

        let type_script = { sudt_scripts.lock().get(&sudt_script_hash).cloned() };

        generate_withdrawal_output(req, &rollup_context, &L2Block::default(), type_script)
            .map_err(|min_capacity| anyhow!("{} minimal capacity required", min_capacity))?;

        Ok(())
    };

    Box::new(verifier)
}

fn build_withdrawal_lock(
    req: &WithdrawalRequest,
    rollup_context: &RollupContext,
    block: &L2Block,
) -> Script {
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

    Script::new_builder()
        .code_hash(rollup_context.rollup_config.withdrawal_script_type_hash())
        .hash_type(ScriptHashType::Type.into())
        .args(lock_args.pack())
        .build()
}

fn build_finalized_custodian_lock(rollup_context: &RollupContext) -> Script {
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
}

fn generate_withdrawal_output(
    req: &WithdrawalRequest,
    rollup_context: &RollupContext,
    block: &L2Block,
    type_script: Option<Script>,
) -> std::result::Result<(CellOutput, Bytes), u64> {
    let req_ckb: u64 = req.raw().capacity().unpack();
    let lock = build_withdrawal_lock(req, rollup_context, block);
    let (type_, data) = match type_script {
        Some(type_) => (Some(type_).pack(), req.raw().amount().as_bytes()),
        None => (None::<Script>.pack(), Bytes::new()),
    };

    let size = 8 + data.len() + type_.as_slice().len() + lock.as_slice().len();
    let min_capacity = size as u64 * 100_000_000;

    if req_ckb < min_capacity {
        return Err(min_capacity);
    }

    let withdrawal = CellOutput::new_builder()
        .capacity(req_ckb.pack())
        .lock(lock)
        .type_(type_)
        .build();

    Ok((withdrawal, data))
}

fn generate_finalized_custodian(
    rollup_context: &RollupContext,
    amount: u128,
    type_: Script,
) -> (CellOutput, Bytes) {
    let lock = build_finalized_custodian_lock(rollup_context);
    let data = amount.pack();

    let capacity = {
        let size = 8 + data.as_slice().len() + type_.as_slice().len() + lock.as_slice().len();
        size as u64 * 100000000u64
    };

    let output = CellOutput::new_builder()
        .capacity(capacity.pack())
        .type_(Some(type_).pack())
        .lock(lock)
        .build();

    (output, data.as_bytes())
}
