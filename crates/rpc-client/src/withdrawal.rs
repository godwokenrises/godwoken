use anyhow::{bail, Result};
use ckb_types::prelude::{Entity, Reader};
use gw_types::bytes::Bytes;
use gw_types::core::{ScriptHashType, Timepoint};
use gw_types::offchain::{CellInfo, CompatibleFinalizedTimepoint};
use gw_types::packed::{
    Byte32, Script, ScriptReader, WithdrawalLockArgs, WithdrawalLockArgsReader,
};
use gw_types::prelude::{Pack, Unpack};

pub fn verify_unlockable_to_owner(
    info: &CellInfo,
    compatible_finalized_timepoint: &CompatibleFinalizedTimepoint,
    l1_sudt_script_hash: &Byte32,
) -> Result<()> {
    verify_l1_sudt_script(info, l1_sudt_script_hash)?;
    verify_finalized_owner_lock(info, compatible_finalized_timepoint)
}

fn verify_l1_sudt_script(info: &CellInfo, l1_sudt_script_hash: &Byte32) -> Result<()> {
    if let Some(sudt_type) = info.output.type_().to_opt() {
        if info.data.len() < ckb_types::packed::Uint128::TOTAL_SIZE {
            bail!("invalid l1 sudt data len");
        }

        if &sudt_type.code_hash() != l1_sudt_script_hash
            || sudt_type.hash_type() != ScriptHashType::Type.into()
        {
            bail!("invalid l1 sudt script");
        }
    }

    Ok(())
}

fn verify_finalized_owner_lock(
    info: &CellInfo,
    compatible_finalized_timepoint: &CompatibleFinalizedTimepoint,
) -> Result<()> {
    let args: Bytes = info.output.lock().args().unpack();

    let lock_args_end = 32 + WithdrawalLockArgs::TOTAL_SIZE;
    let owner_lock_start = lock_args_end + 4; // u32 owner lock length
    if args.len() <= owner_lock_start {
        bail!("no owner lock");
    }

    let lock_args = match WithdrawalLockArgsReader::verify(&args.slice(32..lock_args_end), false) {
        Ok(()) => WithdrawalLockArgs::new_unchecked(args.slice(32..lock_args_end)),
        Err(_) => bail!("invalid withdrawal lock args"),
    };

    if !compatible_finalized_timepoint.is_finalized(&Timepoint::from_full_value(
        lock_args.withdrawal_block_number().unpack(),
    )) {
        println!(
            "lock_args.withdrawal_block_number: {}, compatible_finalized_timepoint: {:?}",
            lock_args.withdrawal_block_number().unpack(),
            compatible_finalized_timepoint,
        );
        bail!("unfinalized withdrawal");
    }

    let mut owner_lock_len_buf = [0u8; 4];
    owner_lock_len_buf.copy_from_slice(&args.slice(lock_args_end..owner_lock_start));
    let owner_lock_len = u32::from_be_bytes(owner_lock_len_buf) as usize;
    if owner_lock_start + owner_lock_len != args.len() {
        bail!("invalid owner lock len");
    }

    let owner_lock = match ScriptReader::verify(&args.slice(owner_lock_start..args.len()), false) {
        Ok(()) => Script::new_unchecked(args.slice(owner_lock_start..args.len())),
        Err(_) => bail!("invalid owner lock"),
    };

    if owner_lock.hash().pack() != lock_args.owner_lock_hash() {
        bail!("owner lock not match");
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use gw_common::h256_ext::H256Ext;
    use gw_common::H256;
    use gw_types::core::{ScriptHashType, Timepoint};
    use gw_types::offchain::{CellInfo, CompatibleFinalizedTimepoint};
    use gw_types::packed::{CellOutput, Script, WithdrawalLockArgs};
    use gw_types::prelude::{Builder, Entity, Pack};

    use super::{verify_finalized_owner_lock, verify_l1_sudt_script};

    #[test]
    fn test_verify_finalized_owner_lock() {
        let owner_lock = Script::new_builder()
            .code_hash(H256::from_u32(1).pack())
            .hash_type(ScriptHashType::Type.into())
            .args(vec![2u8; 32].pack())
            .build();

        let rollup_type_hash = [3u8; 32];

        let finalized_block_number = 100u64;
        let last_finalized_timepoint = Timepoint::from_block_number(finalized_block_number);
        let compatible_finalized_timepoint =
            CompatibleFinalizedTimepoint::from_block_number(finalized_block_number, 0);
        let lock_args = WithdrawalLockArgs::new_builder()
            .owner_lock_hash(owner_lock.hash().pack())
            .withdrawal_block_number(last_finalized_timepoint.full_value().pack())
            .build();

        let mut args = rollup_type_hash.to_vec();
        args.extend_from_slice(&lock_args.as_bytes());
        args.extend_from_slice(&(owner_lock.as_bytes().len() as u32).to_be_bytes());
        args.extend_from_slice(&owner_lock.as_bytes());

        let lock = Script::new_builder().args(args.pack()).build();
        let info = CellInfo {
            output: CellOutput::new_builder().lock(lock).build(),
            ..Default::default()
        };
        verify_finalized_owner_lock(&info, &compatible_finalized_timepoint).expect("pass");

        // # no owner lock
        let mut args = rollup_type_hash.to_vec();
        args.extend_from_slice(&lock_args.as_bytes());
        args.extend_from_slice(&(owner_lock.as_bytes().len() as u32).to_be_bytes());

        let lock = Script::new_builder().args(args.pack()).build();
        let info = CellInfo {
            output: CellOutput::new_builder().lock(lock).build(),
            ..Default::default()
        };
        let err = verify_finalized_owner_lock(&info, &compatible_finalized_timepoint).unwrap_err();
        assert!(err.to_string().contains("no owner lock"));

        // # invalid withdrawal lock args
        // NOTE: Only wrong len can cause ivnalid withdrawal lock args. Since we already ensure
        // withdrawal lock args len, no way to create invalid withdrawal lock args.

        // # unfinalized
        let err_lock_args = lock_args
            .clone()
            .as_builder()
            .withdrawal_block_number((last_finalized_timepoint.full_value() + 1).pack())
            .build();

        let mut args = rollup_type_hash.to_vec();
        args.extend_from_slice(&err_lock_args.as_bytes());
        args.extend_from_slice(&(owner_lock.as_bytes().len() as u32).to_be_bytes());
        args.extend_from_slice(&owner_lock.as_bytes());

        let lock = Script::new_builder().args(args.pack()).build();
        let info = CellInfo {
            output: CellOutput::new_builder().lock(lock).build(),
            ..Default::default()
        };
        let err = verify_finalized_owner_lock(&info, &compatible_finalized_timepoint).unwrap_err();
        assert!(err.to_string().contains("unfinalized"));

        // # invalid owner lock end
        let mut args = rollup_type_hash.to_vec();
        args.extend_from_slice(&lock_args.as_bytes());
        args.extend_from_slice(&(owner_lock.as_bytes().len() as u32 + 1).to_be_bytes());
        args.extend_from_slice(&owner_lock.as_bytes());

        let lock = Script::new_builder().args(args.pack()).build();
        let info = CellInfo {
            output: CellOutput::new_builder().lock(lock).build(),
            ..Default::default()
        };
        let err = verify_finalized_owner_lock(&info, &compatible_finalized_timepoint).unwrap_err();
        assert!(err.to_string().contains("invalid owner lock len"));

        // # invalid owner lock
        let mut args = rollup_type_hash.to_vec();
        args.extend_from_slice(&lock_args.as_bytes());
        args.extend_from_slice(&(owner_lock.as_bytes().len() as u32).to_be_bytes());
        args.extend_from_slice(&vec![1u8; owner_lock.as_bytes().len() as usize]);

        let lock = Script::new_builder().args(args.pack()).build();
        let info = CellInfo {
            output: CellOutput::new_builder().lock(lock).build(),
            ..Default::default()
        };
        let err = verify_finalized_owner_lock(&info, &compatible_finalized_timepoint).unwrap_err();
        assert!(err.to_string().contains("invalid owner lock"));

        // # owner lock not match
        let err_owner_lock = Script::new_builder()
            .code_hash(H256::from_u32(5).pack())
            .args(vec![7u8; 32].pack())
            .build();
        let mut args = rollup_type_hash.to_vec();
        args.extend_from_slice(&lock_args.as_bytes());
        args.extend_from_slice(&(err_owner_lock.as_bytes().len() as u32).to_be_bytes());
        args.extend_from_slice(&err_owner_lock.as_bytes());

        let lock = Script::new_builder().args(args.pack()).build();
        let info = CellInfo {
            output: CellOutput::new_builder().lock(lock).build(),
            ..Default::default()
        };
        let err = verify_finalized_owner_lock(&info, &compatible_finalized_timepoint).unwrap_err();
        assert!(err.to_string().contains("owner lock not match"));
    }

    #[test]
    fn test_verify_l1_sudt_script() {
        let rollup_type_hash = [3u8; 32];

        let owner_lock = Script::new_builder()
            .code_hash(H256::from_u32(1).pack())
            .hash_type(ScriptHashType::Type.into())
            .args(vec![2u8; 32].pack())
            .build();

        let l1_sudt = Script::new_builder()
            .code_hash(H256::from_u32(3).pack())
            .hash_type(ScriptHashType::Type.into())
            .args(vec![4u8; 32].pack())
            .build();

        let last_finalized_timepoint = Timepoint::from_block_number(100);
        let lock_args = WithdrawalLockArgs::new_builder()
            .owner_lock_hash(owner_lock.hash().pack())
            .withdrawal_block_number(last_finalized_timepoint.full_value().pack())
            .build();

        let mut args = rollup_type_hash.to_vec();
        args.extend_from_slice(&lock_args.as_bytes());
        args.extend_from_slice(&(owner_lock.as_bytes().len() as u32).to_be_bytes());
        args.extend_from_slice(&owner_lock.as_bytes());

        let lock = Script::new_builder().args(args.pack()).build();
        let info = CellInfo {
            output: CellOutput::new_builder()
                .lock(lock)
                .type_(Some(l1_sudt.clone()).pack())
                .build(),
            data: 100u128.pack().as_bytes(),
            ..Default::default()
        };
        verify_l1_sudt_script(&info, &l1_sudt.code_hash()).expect("pass");

        // # invalid data len
        let err_info = CellInfo {
            output: info.output.clone(),
            data: 100u64.pack().as_bytes(),
            out_point: info.out_point.clone(),
        };
        let err = verify_l1_sudt_script(&err_info, &l1_sudt.code_hash()).unwrap_err();
        assert!(err.to_string().contains("invalid l1 sudt data len"));

        // # wrong l1 sudt code hash
        let err = verify_l1_sudt_script(&info, &[10u8; 32].pack()).unwrap_err();
        assert!(err.to_string().contains("invalid l1 sudt script"));

        // # wrong hash type
        let err_l1_sudt = l1_sudt
            .as_builder()
            .hash_type(ScriptHashType::Data.into())
            .build();
        let info = CellInfo {
            output: info
                .output
                .as_builder()
                .type_(Some(err_l1_sudt.clone()).pack())
                .build(),
            data: info.data.clone(),
            out_point: info.out_point,
        };

        let err = verify_l1_sudt_script(&info, &err_l1_sudt.hash().pack()).unwrap_err();
        assert!(err.to_string().contains("invalid l1 sudt script"));
    }
}
