use std::collections::HashMap;

use anyhow::Result;
use gw_common::{CKB_SUDT_SCRIPT_ARGS, H256};
use gw_rpc_client::{QueryResult, RPCClient};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    offchain::{CollectedCustodianCells, DepositInfo, RollupContext, WithdrawalsAmount},
    packed::{CellOutput, CustodianLockArgs, DepositLockArgs, Script, WithdrawalRequest},
    prelude::*,
};

pub fn to_custodian_cell(
    rollup_context: &RollupContext,
    block_hash: &H256,
    block_number: u64,
    deposit_info: &DepositInfo,
) -> Result<(CellOutput, Bytes), u128> {
    let lock_args: Bytes = {
        let deposit_lock_args = {
            let lock_args: Bytes = deposit_info.cell.output.lock().args().unpack();
            DepositLockArgs::new_unchecked(lock_args.slice(32..))
        };

        let custodian_lock_args = CustodianLockArgs::new_builder()
            .deposit_block_hash(Into::<[u8; 32]>::into(*block_hash).pack())
            .deposit_block_number(block_number.pack())
            .deposit_lock_args(deposit_lock_args)
            .build();

        let rollup_type_hash = rollup_context.rollup_script_hash.as_slice().iter();
        rollup_type_hash
            .chain(custodian_lock_args.as_slice().iter())
            .cloned()
            .collect()
    };
    let lock = Script::new_builder()
        .code_hash(rollup_context.rollup_config.custodian_script_type_hash())
        .hash_type(ScriptHashType::Type.into())
        .args(lock_args.pack())
        .build();

    // Use custodian lock
    let output = {
        let builder = deposit_info.cell.output.clone().as_builder();
        builder.lock(lock).build()
    };
    let data = deposit_info.cell.data.clone();

    // Check capacity
    match output.occupied_capacity(data.len()) {
        Ok(capacity) if capacity > deposit_info.cell.output.capacity().unpack() => {
            return Err(capacity as u128);
        }
        // Overflow
        Err(err) => {
            log::debug!("calculate occupied capacity {}", err);
            return Err(u64::MAX as u128 + 1);
        }
        _ => (),
    }

    Ok((output, data))
}

#[derive(Debug, Clone)]
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
    pub async fn build_from_withdrawals<WithdrawalIter: Iterator<Item = WithdrawalRequest>>(
        rpc_client: &RPCClient,
        withdrawals: WithdrawalIter,
        rollup_context: &RollupContext,
        last_finalized_block_number: u64,
    ) -> Result<QueryResult<Self>> {
        let total_withdrawal_amount = sum(withdrawals);
        let available_custodians = rpc_client
            .query_finalized_custodian_cells(
                &total_withdrawal_amount,
                calc_ckb_custodian_min_capacity(rollup_context),
                last_finalized_block_number,
            )
            .await?
            .map(|custodian_cells| (&custodian_cells).into());
        Ok(available_custodians)
    }
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
                if sudt_script_hash ==
                    CKB_SUDT_SCRIPT_ARGS {
                        let account = withdrawal.raw().account_script_hash();
                        log::warn!("{} withdrawal request non-zero sudt amount but it's type hash ckb, ignore this amount", account);
                    }
                    else{
                        let total_sudt_amount = total_amount.sudt.entry(sudt_script_hash).or_insert(0u128);
                        *total_sudt_amount = total_sudt_amount.saturating_add(sudt_amount);
                    }
            }

            total_amount
        }
    )
}

pub fn calc_ckb_custodian_min_capacity(rollup_context: &RollupContext) -> u64 {
    let lock = build_finalized_custodian_lock(rollup_context);
    (8 + lock.as_slice().len() as u64).saturating_mul(100000000)
}

pub fn build_finalized_custodian_lock(rollup_context: &RollupContext) -> Script {
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
