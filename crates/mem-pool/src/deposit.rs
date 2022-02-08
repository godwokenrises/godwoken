use anyhow::{anyhow, Result};
use gw_common::{h256_ext::H256Ext, H256};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    offchain::{DepositInfo, RollupContext},
    packed::DepositLockArgs,
    prelude::*,
};
use gw_utils::since::{LockValue, Since};

use crate::custodian::to_custodian_cell;

/// check and reject invalid deposit cells
pub fn sanitize_deposit_cells(
    ctx: &RollupContext,
    unsanitize_deposits: Vec<DepositInfo>,
) -> Vec<DepositInfo> {
    let mut deposit_cells = Vec::with_capacity(unsanitize_deposits.len());
    for cell in unsanitize_deposits {
        // check deposit lock
        // the lock should be correct unless the upstream ckb-indexer has bugs
        if let Err(err) = check_deposit_cell(ctx, &cell) {
            log::debug!("[sanitize deposit cell] {}", err);
            continue;
        }
        deposit_cells.push(cell);
    }
    deposit_cells
}

/// we only package deposit cells with valid cancel timeout, to prevent conflict with user's unlock
fn check_deposit_cell_cancel_timeout(deposit_args: &DepositLockArgs) -> Result<()> {
    const BLOCK_TIMEOUT: u64 = 150; // 150 blocks, about 20 minutes
    const TIMESTAMP_TIMEOUT: u64 = 1_200_000; // 20 minutes
    const EPOCH_TIMEOUT: u64 = 1; // 1 epoch, about 4 hours, this option is supposed not actually used, so we simply set a value

    let cancel_timeout = Since::new(deposit_args.cancel_timeout().unpack());
    if cancel_timeout.flags_is_valid() {
        // reject non relative flag
        if !cancel_timeout.is_relative() {
            return Err(anyhow!(
                "Invalid deposit cancel_time: {}, invalid relative flag",
                deposit_args.cancel_timeout()
            ));
        }

        match cancel_timeout.extract_lock_value().expect("since value") {
            LockValue::BlockNumber(block) if block < BLOCK_TIMEOUT => {
                return Err(anyhow!(
                    "Invalid deposit cancel_time: {}, invalid block timeout, block: {}, required: {}",
                    deposit_args.cancel_timeout(),
                    block,
                    BLOCK_TIMEOUT
                ));
            }
            LockValue::Timestamp(timestamp) if timestamp < TIMESTAMP_TIMEOUT => {
                return Err(anyhow!(
                    "Invalid deposit cancel_time: {}, invalid block timeout, timestamp: {}ms, required: {}ms",
                    deposit_args.cancel_timeout(),
                    timestamp,
                    TIMESTAMP_TIMEOUT
                ));
            }
            LockValue::EpochNumberWithFraction(epoch) if epoch.number() < EPOCH_TIMEOUT => {
                return Err(anyhow!(
                    "Invalid deposit cancel_time: {}, invalid epoch timeout, epoch: {}, required: {}",
                    deposit_args.cancel_timeout(),
                    epoch.number(),
                    EPOCH_TIMEOUT
                ));
            }
            _ => {}
        }
    } else {
        // cancel timeout is invalid, which means user can't unlock it, so we can safely use this cell
        log::debug!(
            "Invalid deposit cancel_time: {}, invalid flag, the deposit is still can be packaged",
            deposit_args.cancel_timeout()
        );
    }
    Ok(())
}

// check deposit cell
fn check_deposit_cell(ctx: &RollupContext, cell: &DepositInfo) -> Result<()> {
    let hash_type = ScriptHashType::Type.into();

    // check deposit lock
    // the lock should be correct unless the upstream ckb-indexer has bugs
    {
        let lock = cell.cell.output.lock();
        if lock.code_hash() != ctx.rollup_config.deposit_script_type_hash()
            || lock.hash_type() != hash_type
        {
            return Err(anyhow!(
                "Invalid deposit lock, expect code_hash: {}, hash_type: Type, got: {}, {}",
                ctx.rollup_config.deposit_script_type_hash(),
                lock.code_hash(),
                lock.hash_type()
            ));
        }
        let args: Bytes = lock.args().unpack();
        if args.len() < 32 {
            return Err(anyhow!(
                "Invalid deposit args, expect len: 32, got: {}",
                args.len()
            ));
        }
        if &args[..32] != ctx.rollup_script_hash.as_slice() {
            return Err(anyhow!(
                "Invalid deposit args, expect rollup_script_hash: {}, got: {}",
                hex::encode(ctx.rollup_script_hash.as_slice()),
                hex::encode(&args[..32])
            ));
        }

        // check deposit args
        let deposit_args = DepositLockArgs::from_slice(&args[32..])?;
        check_deposit_cell_cancel_timeout(&deposit_args)?;
    }

    // check sUDT
    // sUDT may be invalid, this may caused by malicious user
    if let Some(type_) = cell.cell.output.type_().to_opt() {
        if type_.code_hash() != ctx.rollup_config.l1_sudt_script_type_hash()
            || type_.hash_type() != hash_type
        {
            return Err(anyhow!(
                "Invalid deposit sUDT, expect code_hash: {}, hash_type: Type, got: {}, {}",
                ctx.rollup_config.l1_sudt_script_type_hash(),
                type_.code_hash(),
                type_.hash_type()
            ));
        }
    }

    // check request
    // request deposit account maybe invalid, this may caused by malicious user
    {
        let script = cell.request.script();
        if script.hash_type() != ScriptHashType::Type.into() {
            return Err(anyhow!(
                "Invalid deposit account script: unexpected hash_type: Data"
            ));
        }
        if ctx
            .rollup_config
            .allowed_eoa_type_hashes()
            .into_iter()
            .all(|type_hash| script.code_hash() != type_hash.hash())
        {
            return Err(anyhow!(
                "Invalid deposit account script: unknown code_hash: {:?}",
                hex::encode(script.code_hash().as_slice())
            ));
        }
        let args: Bytes = script.args().unpack();
        if args.len() < 32 {
            return Err(anyhow!(
                "Invalid deposit account args, expect len: 32, got: {}",
                args.len()
            ));
        }
        if &args[..32] != ctx.rollup_script_hash.as_slice() {
            return Err(anyhow!(
                "Invalid deposit account args, expect rollup_script_hash: {}, got: {}",
                hex::encode(ctx.rollup_script_hash.as_slice()),
                hex::encode(&args[..32])
            ));
        }
    }

    // check capacity (use dummy block hash and number)
    if let Err(minimal_capacity) = to_custodian_cell(ctx, &H256::one(), 1, cell) {
        let deposit_capacity = cell.cell.output.capacity().unpack();
        return Err(anyhow!(
            "Invalid deposit capacity, unable to generate custodian, minimal required: {}, got: {}",
            minimal_capacity,
            deposit_capacity
        ));
    }

    Ok(())
}
