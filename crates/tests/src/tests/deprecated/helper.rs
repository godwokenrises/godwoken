#![allow(clippy::mutable_key_type)]

use anyhow::{anyhow, bail, Result};
use gw_common::H256;
use gw_config::ContractsCellDep;
use gw_generator::error::WithdrawalError;
use gw_mem_pool::custodian::{
    build_finalized_custodian_lock, calc_ckb_custodian_min_capacity, generate_finalized_custodian,
    sum_withdrawals, AvailableCustodians,
};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    offchain::{CollectedCustodianCells, InputCellInfo, RollupContext},
    packed::{
        CellDep, CellInput, CellOutput, L2Block, Script, WithdrawalLockArgs, WithdrawalRequest,
        WithdrawalRequestExtra,
    },
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

#[derive(Clone)]
struct CkbCustodian {
    capacity: u128,
    balance: u128,
    min_capacity: u64,
}

struct SudtCustodian {
    capacity: u64,
    balance: u128,
    script: Script,
}

pub struct Generator<'a> {
    rollup_context: &'a RollupContext,
    ckb_custodian: CkbCustodian,
    sudt_custodians: HashMap<[u8; 32], SudtCustodian>,
    withdrawals: Vec<(CellOutput, Bytes)>,
}

impl<'a> Generator<'a> {
    pub fn new(
        rollup_context: &'a RollupContext,
        available_custodians: AvailableCustodians,
    ) -> Self {
        let mut total_sudt_capacity = 0u128;
        let mut sudt_custodians = HashMap::new();

        for (sudt_type_hash, (balance, type_script)) in available_custodians.sudt.into_iter() {
            let (change, _data) =
                generate_finalized_custodian(rollup_context, balance, type_script.clone());

            let sudt_custodian = SudtCustodian {
                capacity: change.capacity().unpack(),
                balance,
                script: type_script,
            };

            total_sudt_capacity =
                total_sudt_capacity.saturating_add(sudt_custodian.capacity as u128);
            sudt_custodians.insert(sudt_type_hash, sudt_custodian);
        }

        let ckb_custodian_min_capacity = calc_ckb_custodian_min_capacity(rollup_context);
        let ckb_custodian_capacity = available_custodians
            .capacity
            .saturating_sub(total_sudt_capacity);
        let ckb_balance = ckb_custodian_capacity.saturating_sub(ckb_custodian_min_capacity as u128);

        let ckb_custodian = CkbCustodian {
            capacity: ckb_custodian_capacity,
            balance: ckb_balance,
            min_capacity: ckb_custodian_min_capacity,
        };

        Generator {
            rollup_context,
            ckb_custodian,
            sudt_custodians,
            withdrawals: Default::default(),
        }
    }

    pub fn verified_output(
        &self,
        req_extra: &WithdrawalRequestExtra,
        block: &L2Block,
    ) -> Result<(CellOutput, Bytes)> {
        // Verify finalized custodian exists
        let req = req_extra.request();
        let req_sudt: u128 = req.raw().amount().unpack();
        let sudt_type_hash: [u8; 32] = req.raw().sudt_script_hash().unpack();
        if 0 != req_sudt && !self.sudt_custodians.contains_key(&sudt_type_hash) {
            return Err(anyhow!("no finalized sudt custodian for {}", req));
        }

        // Verify minimal capacity
        let sudt_script = {
            let sudt_custodian = self.sudt_custodians.get(&sudt_type_hash);
            sudt_custodian.map(|sudt| sudt.script.to_owned())
        };
        let block_hash: H256 = block.hash().into();
        let block_number = block.raw().number().unpack();
        let output = match build_withdrawal_cell_output(
            self.rollup_context,
            req_extra,
            &block_hash,
            block_number,
            sudt_script,
        ) {
            Ok(output) => output,
            Err(WithdrawalCellError::OwnerLock(lock_hash)) => {
                bail!("owner lock not match hash {}", lock_hash.pack())
            }
            Err(WithdrawalCellError::MinCapacity { min, req: _ }) => {
                bail!("{} minimal capacity for {}", min, req)
            }
        };

        self.verify_remained_amount(&req).map(|_| output)
    }

    pub fn verify_remained_amount(&self, req: &WithdrawalRequest) -> Result<()> {
        // Verify remained sudt
        let mut ckb_custodian = self.ckb_custodian.clone();
        let sudt_type_hash: [u8; 32] = req.raw().sudt_script_hash().unpack();
        let req_sudt: u128 = req.raw().amount().unpack();
        if 0 != req_sudt {
            let sudt_custodian = match self.sudt_custodians.get(&sudt_type_hash) {
                Some(custodian) => custodian,
                None => {
                    return Err(anyhow!(
                        "Finalized simple UDT custodian cell is not enough to withdraw"
                    ))
                }
            };

            let remained = sudt_custodian
                .balance
                .checked_sub(req_sudt)
                .ok_or_else(|| {
                    anyhow!("Finalized simple UDT custodian cell is not enough to withdraw")
                })?;

            // Consume all remained sudt, give sudt custodian capacity back to ckb custodian
            if 0 == remained {
                // If ckb custodian is already consumed
                if 0 == ckb_custodian.capacity {
                    ckb_custodian.capacity = sudt_custodian.capacity as u128;
                    ckb_custodian.balance =
                        (sudt_custodian.capacity - ckb_custodian.min_capacity) as u128;
                } else {
                    ckb_custodian.capacity += sudt_custodian.capacity as u128;
                    ckb_custodian.balance += sudt_custodian.capacity as u128;
                }
            }
        }

        // Verify remained ckb
        let req_ckb = req.raw().capacity().unpack() as u128;
        match ckb_custodian.balance.checked_sub(req_ckb) {
            Some(_) => Ok(()),
            // Consume all remained ckb
            None if req_ckb == ckb_custodian.capacity => Ok(()),
            // No able to cover withdrawal cell and ckb custodian change
            None => Err(anyhow!(
                "Finalized CKB custodian cell is not enough to withdraw"
            )),
        }
    }

    pub fn include_and_verify(
        &mut self,
        req_extra: &WithdrawalRequestExtra,
        block: &L2Block,
    ) -> Result<()> {
        let verified_output = self.verified_output(req_extra, block)?;
        let ckb_custodian = &mut self.ckb_custodian;

        // Update custodians according to verified output
        let req = req_extra.request();
        let req_sudt: u128 = req.raw().amount().unpack();
        if 0 != req_sudt {
            let sudt_type_hash: [u8; 32] = req.raw().sudt_script_hash().unpack();
            let sudt_custodian = match self.sudt_custodians.get_mut(&sudt_type_hash) {
                Some(custodian) => custodian,
                None => return Err(anyhow!("unexpected sudt not found for verified {}", req)),
            };

            match sudt_custodian.balance.checked_sub(req_sudt) {
                Some(remaind) => sudt_custodian.balance = remaind,
                None => return Err(anyhow!("unexpected sudt overflow for verified {}", req)),
            }

            // Consume all remaind sudt, give sudt custodian capacity back to ckb custodian
            if 0 == sudt_custodian.balance {
                if 0 == ckb_custodian.capacity {
                    ckb_custodian.capacity = sudt_custodian.capacity as u128;
                    ckb_custodian.balance =
                        (sudt_custodian.capacity - ckb_custodian.min_capacity) as u128;
                } else {
                    ckb_custodian.capacity += sudt_custodian.capacity as u128;
                    ckb_custodian.balance += sudt_custodian.capacity as u128;
                }
                sudt_custodian.capacity = 0;
            }
        }

        let req_ckb = req.raw().capacity().unpack() as u128;
        match ckb_custodian.balance.checked_sub(req_ckb) {
            Some(remaind) => {
                ckb_custodian.capacity -= req_ckb;
                ckb_custodian.balance = remaind;
            }
            // Consume all remaind ckb
            None if req_ckb == ckb_custodian.capacity => {
                ckb_custodian.capacity = 0;
                ckb_custodian.balance = 0;
            }
            None => return Err(anyhow!("unexpected capacity overflow for verified {}", req)),
        }

        self.withdrawals.push(verified_output);
        Ok(())
    }

    pub fn finish(self) -> Vec<(CellOutput, Bytes)> {
        let mut outputs = self.withdrawals;
        let custodian_lock = build_finalized_custodian_lock(self.rollup_context);

        // Generate sudt custodian changes
        let sudt_changes = {
            let custodians = self.sudt_custodians.into_iter();
            custodians.filter(|(_, custodian)| 0 != custodian.capacity && 0 != custodian.balance)
        };
        for custodian in sudt_changes.map(|(_, c)| c) {
            let output = CellOutput::new_builder()
                .capacity(custodian.capacity.pack())
                .type_(Some(custodian.script).pack())
                .lock(custodian_lock.clone())
                .build();

            outputs.push((output, custodian.balance.pack().as_bytes()));
        }

        // Generate ckb custodian change
        let build_ckb_output = |capacity: u64| -> (CellOutput, Bytes) {
            let output = CellOutput::new_builder()
                .capacity(capacity.pack())
                .lock(custodian_lock.clone())
                .build();
            (output, Bytes::new())
        };
        if 0 != self.ckb_custodian.capacity {
            if self.ckb_custodian.capacity < u64::MAX as u128 {
                outputs.push(build_ckb_output(self.ckb_custodian.capacity as u64));
                return outputs;
            }

            // Fit ckb-indexer output_capacity_range [inclusive, exclusive]
            let max_capacity = u64::MAX - 1;
            let ckb_custodian = self.ckb_custodian;
            let mut remaind = ckb_custodian.capacity;
            while remaind > 0 {
                let max = remaind.saturating_sub(ckb_custodian.min_capacity as u128);
                match max.checked_sub(max_capacity as u128) {
                    Some(cap) => {
                        outputs.push(build_ckb_output(max_capacity));
                        remaind = cap.saturating_add(ckb_custodian.min_capacity as u128);
                    }
                    None if max.saturating_add(ckb_custodian.min_capacity as u128)
                        > max_capacity as u128 =>
                    {
                        let max = max.saturating_add(ckb_custodian.min_capacity as u128);
                        let half = max / 2;
                        outputs.push(build_ckb_output(half as u64));
                        outputs.push(build_ckb_output(max.saturating_sub(half) as u64));
                        remaind = 0;
                    }
                    None => {
                        outputs.push(build_ckb_output(
                            (max as u64).saturating_add(ckb_custodian.min_capacity),
                        ));
                        remaind = 0;
                    }
                }
            }
        }

        outputs
    }
}

#[derive(Debug)]
pub enum WithdrawalCellError {
    MinCapacity { min: u128, req: u64 },
    OwnerLock(H256),
}

impl From<WithdrawalCellError> for gw_generator::Error {
    fn from(err: WithdrawalCellError) -> Self {
        match err {
            WithdrawalCellError::MinCapacity { min, req } => {
                WithdrawalError::InsufficientCapacity {
                    expected: min,
                    actual: req,
                }
                .into()
            }
            WithdrawalCellError::OwnerLock(hash) => WithdrawalError::OwnerLock(hash.pack()).into(),
        }
    }
}

pub fn build_withdrawal_cell_output(
    rollup_context: &RollupContext,
    req: &WithdrawalRequestExtra,
    block_hash: &H256,
    block_number: u64,
    opt_asset_script: Option<Script>,
) -> Result<(CellOutput, Bytes), WithdrawalCellError> {
    let withdrawal_capacity: u64 = req.raw().capacity().unpack();
    let lock_args: Bytes = {
        let withdrawal_lock_args = WithdrawalLockArgs::new_builder()
            .account_script_hash(req.raw().account_script_hash())
            .withdrawal_block_hash(Into::<[u8; 32]>::into(*block_hash).pack())
            .withdrawal_block_number(block_number.pack())
            .owner_lock_hash(req.raw().owner_lock_hash())
            .build();

        let mut args = Vec::new();
        args.extend_from_slice(rollup_context.rollup_script_hash.as_slice());
        args.extend_from_slice(withdrawal_lock_args.as_slice());
        let owner_lock = req.owner_lock();
        let owner_lock_hash: [u8; 32] = req.raw().owner_lock_hash().unpack();
        if owner_lock_hash != owner_lock.hash() {
            return Err(WithdrawalCellError::OwnerLock(owner_lock_hash.into()));
        }
        args.extend_from_slice(&(owner_lock.as_slice().len() as u32).to_be_bytes());
        args.extend_from_slice(owner_lock.as_slice());

        Bytes::from(args)
    };

    let lock = Script::new_builder()
        .code_hash(rollup_context.rollup_config.withdrawal_script_type_hash())
        .hash_type(ScriptHashType::Type.into())
        .args(lock_args.pack())
        .build();

    let (type_, data) = match opt_asset_script {
        Some(type_) => (Some(type_).pack(), req.raw().amount().as_bytes()),
        None => (None::<Script>.pack(), Bytes::new()),
    };

    let output = CellOutput::new_builder()
        .capacity(withdrawal_capacity.pack())
        .type_(type_)
        .lock(lock)
        .build();

    match output.occupied_capacity(data.len()) {
        Ok(min_capacity) if min_capacity > withdrawal_capacity => {
            Err(WithdrawalCellError::MinCapacity {
                min: min_capacity as u128,
                req: req.raw().capacity().unpack(),
            })
        }
        Err(err) => {
            println!("calculate withdrawal capacity {}", err); // Overflow
            Err(WithdrawalCellError::MinCapacity {
                min: u64::MAX as u128 + 1,
                req: req.raw().capacity().unpack(),
            })
        }
        _ => Ok((output, data)),
    }
}
