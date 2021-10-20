use anyhow::{anyhow, Result};
use gw_common::H256;
use gw_types::{
    bytes::Bytes,
    offchain::RollupContext,
    packed::{CellOutput, L2Block, Script, WithdrawalRequest},
    prelude::*,
};

use std::collections::HashMap;

use crate::custodian::{
    build_finalized_custodian_lock, calc_ckb_custodian_min_capacity, generate_finalized_custodian,
    AvailableCustodians,
};

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

    pub fn withdrawals(&self) -> &[(CellOutput, Bytes)] {
        &self.withdrawals
    }

    pub fn verified_output(
        &self,
        req: &WithdrawalRequest,
        block: &L2Block,
    ) -> Result<(CellOutput, Bytes)> {
        // Verify finalized custodian exists
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
        let output = gw_generator::Generator::build_withdrawal_cell_output(
            self.rollup_context,
            req,
            &block_hash,
            block_number,
            sudt_script,
        )
        .map_err(|min_capacity| anyhow!("{} minimal capacity for {}", min_capacity, req))?;

        self.verify_remained_amount(req).map(|_| output)
    }

    pub fn verify_remained_amount(&self, req: &WithdrawalRequest) -> Result<()> {
        // Verify remaind sudt
        let mut ckb_custodian = self.ckb_custodian.clone();
        let sudt_type_hash: [u8; 32] = req.raw().sudt_script_hash().unpack();
        let req_sudt: u128 = req.raw().amount().unpack();
        if 0 != req_sudt {
            let sudt_custodian = match self.sudt_custodians.get(&sudt_type_hash) {
                Some(custodian) => custodian,
                None => return Err(anyhow!("no finalized sudt custodian for {}", req)),
            };

            let remaind = sudt_custodian
                .balance
                .checked_sub(req_sudt)
                .ok_or_else(|| anyhow!("no enough custodian sudt for {}", req))?;

            // Consume all remaind sudt, give sudt custodian capacity back to ckb custodian
            if 0 == remaind {
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

        // Verify remaind ckb
        let req_ckb = req.raw().capacity().unpack() as u128;
        match ckb_custodian.balance.checked_sub(req_ckb) {
            Some(_) => Ok(()),
            // Consume all remaind ckb
            None if req_ckb == ckb_custodian.capacity => Ok(()),
            // No able to cover withdrawal cell and ckb custodian change
            None => Err(anyhow!(
                "no enough finalized custodian capacity, custodian ckb: {}, required ckb: {}",
                ckb_custodian.capacity,
                req_ckb
            )),
        }
    }

    pub fn include_and_verify(&mut self, req: &WithdrawalRequest, block: &L2Block) -> Result<()> {
        let verified_output = self.verified_output(req, block)?;
        let ckb_custodian = &mut self.ckb_custodian;

        // Update custodians according to verified output
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

            let ckb_custodian = self.ckb_custodian;
            let mut remaind = ckb_custodian.capacity;
            while remaind > 0 {
                let max = remaind.saturating_sub(ckb_custodian.min_capacity as u128);
                match max.checked_sub(u64::MAX as u128) {
                    Some(cap) => {
                        outputs.push(build_ckb_output(u64::MAX));
                        remaind = cap.saturating_add(ckb_custodian.min_capacity as u128);
                    }
                    None if max.saturating_add(ckb_custodian.min_capacity as u128)
                        > u64::MAX as u128 =>
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
