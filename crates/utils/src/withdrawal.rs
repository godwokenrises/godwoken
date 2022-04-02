use anyhow::{bail, Result};
use gw_types::bytes::Bytes;
use gw_types::packed::{Script, ScriptReader, WithdrawalLockArgs, WithdrawalLockArgsReader};
use gw_types::prelude::{Entity, Reader, Unpack};

pub struct ParsedWithdrawalLockArgs {
    pub rollup_type_hash: [u8; 32],
    pub lock_args: WithdrawalLockArgs,
    pub opt_owner_lock: Option<Script>,
    pub withdraw_to_v1: bool,
}

pub fn parse_lock_args(args: &Bytes) -> Result<ParsedWithdrawalLockArgs> {
    let lock_args_start = 32;
    let lock_args_end = lock_args_start + WithdrawalLockArgs::TOTAL_SIZE;

    let args_len = args.len();
    if args_len < lock_args_end {
        bail!("invalid args len");
    }

    let mut rollup_type_hash = [0u8; 32];
    rollup_type_hash.copy_from_slice(&args.slice(..32));

    let raw_args = args.slice(lock_args_start..lock_args_end);
    let lock_args = match WithdrawalLockArgsReader::verify(&raw_args, false) {
        Ok(()) => WithdrawalLockArgs::new_unchecked(raw_args),
        Err(err) => bail!("invalid args {}", err),
    };

    let owner_lock_start = lock_args_end + 4; // u32 length
    if args_len <= owner_lock_start {
        return Ok(ParsedWithdrawalLockArgs {
            rollup_type_hash,
            lock_args,
            opt_owner_lock: None,
            withdraw_to_v1: false,
        });
    }

    let mut owner_lock_len_buf = [0u8; 4];
    owner_lock_len_buf.copy_from_slice(&args.slice(lock_args_end..owner_lock_start));

    let owner_lock_len = u32::from_be_bytes(owner_lock_len_buf) as usize;
    let owner_lock_end = owner_lock_start + owner_lock_len;
    // Plus one v1 flag byte
    if owner_lock_end != args_len && owner_lock_end + 1 != args_len {
        bail!("invalid args owner lock script len");
    }

    let raw_script = args.slice(owner_lock_start..owner_lock_end);
    let owner_lock = match ScriptReader::verify(&raw_script, false) {
        Ok(()) => Script::new_unchecked(raw_script),
        Err(err) => bail!("invalid args owner lock script {}", err),
    };

    let owner_lock_hash: [u8; 32] = lock_args.owner_lock_hash().unpack();
    if owner_lock.hash() != owner_lock_hash {
        bail!("invalid args owner lock hash");
    }

    let withdraw_to_v1 = owner_lock_end + 1 == args_len && args[owner_lock_end] == 1u8;

    Ok(ParsedWithdrawalLockArgs {
        rollup_type_hash,
        lock_args,
        opt_owner_lock: Some(owner_lock),
        withdraw_to_v1,
    })
}
