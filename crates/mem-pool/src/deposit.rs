use anyhow::{anyhow, Result};
use gw_common::{h256_ext::H256Ext, registry::context::RegistryContext, state::State, H256};
use gw_config::DepositTimeoutConfig;
use gw_store::state::MemStateDB;
use gw_types::core::Timepoint;
use gw_types::{
    bytes::Bytes, core::ScriptHashType, offchain::DepositInfo, packed::DepositLockArgs, prelude::*,
};
use gw_utils::since::{LockValue, Since};
use gw_utils::RollupContext;

use crate::custodian::to_custodian_cell;

/// check and reject invalid deposit cells
pub fn sanitize_deposit_cells(
    ctx: &RollupContext,
    config: &DepositTimeoutConfig,
    unsanitize_deposits: Vec<DepositInfo>,
    state: &MemStateDB,
) -> Vec<DepositInfo> {
    log::debug!(target: "collect-deposit-cells", "sanitize {} deposits", unsanitize_deposits.len());
    let mut deposit_cells = Vec::with_capacity(unsanitize_deposits.len());
    for cell in unsanitize_deposits {
        // check deposit lock
        // the lock should be correct unless the upstream ckb-indexer has bugs
        if let Err(err) = check_deposit_cell(ctx, config, &cell, state) {
            log::debug!(target: "collect-deposit-cells", "invalid deposit cell: {}", err);
            continue;
        }
        deposit_cells.push(cell);
    }
    log::debug!(target: "collect-deposit-cells", "return {} sanitized deposits", deposit_cells.len());
    deposit_cells
}

/// we only package deposit cells with valid cancel timeout, to prevent conflict with user's unlock
fn check_deposit_cell_cancel_timeout(
    config: &DepositTimeoutConfig,
    deposit_args: &DepositLockArgs,
) -> Result<()> {
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
            LockValue::BlockNumber(block) if block < config.deposit_block_timeout => {
                return Err(anyhow!(
                    "Invalid deposit cancel_time: {}, invalid block timeout, block: {}, required: {}",
                    deposit_args.cancel_timeout(),
                    block,
                    config.deposit_block_timeout
                ));
            }
            LockValue::Timestamp(timestamp) if timestamp < config.deposit_timestamp_timeout => {
                return Err(anyhow!(
                    "Invalid deposit cancel_time: {}, invalid block timeout, timestamp: {}ms, required: {}ms",
                    deposit_args.cancel_timeout(),
                    timestamp,
                    config.deposit_timestamp_timeout
                ));
            }
            LockValue::EpochNumberWithFraction(epoch)
                if epoch.number() < config.deposit_epoch_timeout =>
            {
                return Err(anyhow!(
                    "Invalid deposit cancel_time: {}, invalid epoch timeout, epoch: {}, required: {}",
                    deposit_args.cancel_timeout(),
                    epoch.number(),
                    config.deposit_epoch_timeout
                ));
            }
            _ => {}
        }
    } else {
        // cancel timeout is invalid, which means user can't unlock it, so we can safely use this cell
        log::debug!(
            target: "collect-deposit-cells",
            "Invalid deposit cancel_time: {}, invalid flag, the deposit is still can be packaged",
            deposit_args.cancel_timeout()
        );
    }
    Ok(())
}

// check deposit cell
fn check_deposit_cell(
    ctx: &RollupContext,
    config: &DepositTimeoutConfig,
    cell: &DepositInfo,
    state: &MemStateDB,
) -> Result<()> {
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
        check_deposit_cell_cancel_timeout(config, &deposit_args)?;
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
        // godwoken only allow to deposit to an EOA account
        // check code hash of deposit reqeust is an EOA type hash
        if !ctx
            .rollup_config
            .allowed_eoa_type_hashes()
            .into_iter()
            .any(|type_hash| type_hash.hash() == script.code_hash())
        {
            return Err(
                anyhow!("Invalid deposit account script: script.code_hash is not in configured allowed_eoa_type_hashes, script.code_hash: {}",
                    hex::encode(&script.code_hash().as_bytes())
                ),
            );
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

        // try extract address from deposit
        let registry_ctx = RegistryContext::new(
            ctx.rollup_config
                .allowed_eoa_type_hashes()
                .into_iter()
                .collect(),
        );

        match registry_ctx.extract_registry_address_from_deposit(
            cell.request.registry_id().unpack(),
            &script.code_hash(),
            &script.args().raw_data(),
        ) {
            Ok(reg_addr) => {
                //Registry address could be duplicated with a contract account.
                if let Some(script_hash) = state.get_script_hash_by_registry_address(&reg_addr)? {
                    if script.hash() != script_hash.as_slice() {
                        return Err(anyhow!(
                            "The RegistryAddress {:?} was already occupied by script_hash {}, depositing script_hash: {}",
                            hex::encode(reg_addr.to_bytes()),
                            hex::encode(&script_hash.as_slice()),
                            hex::encode(&script.hash()),
                        ));
                    }
                }
            }
            Err(err) => {
                return Err(anyhow!(
                    "Failed to extract registry address from deposit, err: {}",
                    err
                ));
            }
        }
    }

    // check capacity (use dummy block hash and number)
    let dummy_block_timepoint = Timepoint::from_block_number(1);
    if let Err(minimal_capacity) =
        to_custodian_cell(ctx, &H256::one(), &dummy_block_timepoint, cell)
    {
        let deposit_capacity = cell.cell.output.capacity().unpack();
        return Err(anyhow!(
            "Invalid deposit capacity, unable to generate custodian, minimal required: {}, got: {}",
            minimal_capacity,
            deposit_capacity
        ));
    }

    Ok(())
}
