use gw_common::H256;
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    offchain::{DepositInfo, RollupContext},
    packed::{CellOutput, CustodianLockArgs, DepositLockArgs, Script},
    prelude::*,
};

pub fn generate_finalized_custodian(
    rollup_context: &RollupContext,
    amount: u128,
    type_: Script,
) -> (CellOutput, Bytes) {
    let lock = build_finalized_custodian_lock(rollup_context);
    let data = amount.pack().as_bytes();
    let dummy_capacity = 1;
    let output = CellOutput::new_builder()
        .capacity(dummy_capacity.pack())
        .type_(Some(type_).pack())
        .lock(lock)
        .build();
    let capacity = output.occupied_capacity(data.len()).expect("overflow");
    let output = output.as_builder().capacity(capacity.pack()).build();

    (output, data)
}

pub fn calc_ckb_custodian_min_capacity(rollup_context: &RollupContext) -> u64 {
    let lock = build_finalized_custodian_lock(rollup_context);
    let dummy = CellOutput::new_builder()
        .capacity(1u64.pack())
        .lock(lock)
        .build();
    dummy.occupied_capacity(0).expect("overflow")
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
