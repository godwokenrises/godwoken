use ckb_std::debug;
use gw_types::{
    bytes::Bytes,
    packed::{Script, ScriptReader, WithdrawalLockArgs, WithdrawalLockArgsReader},
    prelude::{CalcHash, Entity, Reader, Unpack},
};

use crate::error::Error;

pub struct WithdrawalLockArgsWithOwnerLock {
    pub lock_args: WithdrawalLockArgs,
    pub owner_lock: Script,
}

/// args: rollup_type_hash | withdrawal lock args | owner lock len (optional) | owner lock (optional)
pub fn parse_lock_args(args: &Bytes) -> Result<WithdrawalLockArgsWithOwnerLock, Error> {
    let lock_args_start = 32;
    let lock_args_end = lock_args_start + WithdrawalLockArgs::TOTAL_SIZE;

    let args_len = args.len();
    if args_len < lock_args_end {
        return Err(Error::InvalidArgs);
    }

    let raw_args = args.slice(lock_args_start..lock_args_end);
    let lock_args = match WithdrawalLockArgsReader::verify(&raw_args, false) {
        Ok(()) => WithdrawalLockArgs::new_unchecked(raw_args),
        Err(_) => return Err(Error::InvalidArgs),
    };

    let owner_lock_start = lock_args_end + 4; // u32 length
    if args_len <= owner_lock_start {
        debug!("[parse withdrawal] missing owner lock");
        return Err(Error::InvalidArgs);
    }

    let mut owner_lock_len_buf = [0u8; 4];
    owner_lock_len_buf.copy_from_slice(&args.slice(lock_args_end..owner_lock_start));

    let owner_lock_len = u32::from_be_bytes(owner_lock_len_buf) as usize;
    let owner_lock_end = owner_lock_start + owner_lock_len;
    if owner_lock_end != args_len {
        return Err(Error::InvalidArgs);
    }

    let raw_script = args.slice(owner_lock_start..owner_lock_end);
    let owner_lock = match ScriptReader::verify(&raw_script, false) {
        Ok(()) => Script::new_unchecked(raw_script),
        Err(_) => return Err(Error::InvalidArgs),
    };

    let owner_lock_hash: [u8; 32] = lock_args.owner_lock_hash().unpack();
    if owner_lock.hash() != owner_lock_hash {
        debug!("[parse withdrawal] incorrect owner lock");
        return Err(Error::InvalidArgs);
    }

    Ok(WithdrawalLockArgsWithOwnerLock {
        lock_args,
        owner_lock,
    })
}
