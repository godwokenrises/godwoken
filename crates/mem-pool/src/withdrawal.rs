use anyhow::{anyhow, Result};
use gw_common::CKB_SUDT_SCRIPT_ARGS;
use gw_rpc_client::RPCClient;
use gw_store::transaction::StoreTransaction;
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    offchain::{CollectedCustodianCells, RollupContext},
    packed::{
        CellOutput, CustodianLockArgs, L2Block, Script, WithdrawalLockArgs, WithdrawalRequest,
    },
    prelude::*,
};

use std::collections::HashMap;

#[derive(Debug)]
pub struct AvailableCustodians {
    pub capacity: u128,
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

impl AvailableCustodians {
    pub fn build(
        db: &StoreTransaction,
        rpc_client: &RPCClient,
        withdrawal_requests: &[WithdrawalRequest],
    ) -> Result<Self> {
        if withdrawal_requests.is_empty() {
            Ok(AvailableCustodians::default())
        } else {
            let mut sudt_scripts: HashMap<[u8; 32], Script> = HashMap::new();
            let sudt_custodians = {
                let reqs = withdrawal_requests.iter();
                let sudt_reqs = reqs.filter(|req| {
                    let sudt_script_hash: [u8; 32] = req.raw().sudt_script_hash().unpack();
                    0 != req.raw().amount().unpack() && CKB_SUDT_SCRIPT_ARGS != sudt_script_hash
                });

                let to_hash = sudt_reqs.map(|req| req.raw().sudt_script_hash().unpack());
                let has_script = to_hash.filter_map(|hash: [u8; 32]| {
                    if let Some(script) = sudt_scripts.get(&hash).cloned() {
                        return Some((hash, script));
                    }

                    // Try rpc
                    match smol::block_on(rpc_client.query_verified_custodian_type_script(&hash)) {
                        Ok(opt_script) => opt_script.map(|script| {
                            sudt_scripts.insert(hash, script.clone());
                            (hash, script)
                        }),
                        Err(err) => {
                            log::debug!("get custodian type script err {}", err);
                            None
                        }
                    }
                });

                let to_custodian = has_script.filter_map(|(hash, script)| {
                    match db.get_finalized_custodian_asset(hash.into()) {
                        Ok(custodian_balance) => Some((hash, (custodian_balance, script))),
                        Err(err) => {
                            log::warn!("get custodian err {}", err);
                            None
                        }
                    }
                });
                to_custodian.collect::<HashMap<[u8; 32], (u128, Script)>>()
            };

            let ckb_custodian = match db.get_finalized_custodian_asset(CKB_SUDT_SCRIPT_ARGS.into())
            {
                Ok(balance) => balance,
                Err(err) => {
                    log::warn!("get ckb custodian err {}", err);
                    0
                }
            };

            Ok(AvailableCustodians {
                capacity: ckb_custodian,
                sudt: sudt_custodians,
            })
        }
    }
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

        let ckb_custodian_min_capacity = {
            let lock = build_finalized_custodian_lock(rollup_context);
            (8 + lock.as_slice().len() as u64) * 100000000
        };
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
        let output = generate_withdrawal_output(req, self.rollup_context, block, sudt_script)
            .map_err(|min_capacity| anyhow!("{} minimal capacity for {}", min_capacity, req))?;

        // Verify remaind sudt
        let mut ckb_custodian = self.ckb_custodian.clone();
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
            Some(_) => Ok(output),
            // Consume all remaind ckb
            None if req_ckb == ckb_custodian.capacity => Ok(output),
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
