use alloc::vec::Vec;
use ckb_std::{
    ckb_constants::Source,
    high_level::{load_cell_lock_hash, QueryIter},
};
use gw_common::{CKB_SUDT_SCRIPT_ARGS, H256};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{RollupConfig, Script},
    prelude::*,
};

pub fn search_lock_hashes(owner_lock_hash: &[u8; 32], source: Source) -> Vec<usize> {
    QueryIter::new(load_cell_lock_hash, source)
        .enumerate()
        .filter_map(|(i, lock_hash)| {
            if &lock_hash == owner_lock_hash {
                Some(i)
            } else {
                None
            }
        })
        .collect()
}

pub fn search_lock_hash(owner_lock_hash: &[u8; 32], source: Source) -> Option<usize> {
    QueryIter::new(load_cell_lock_hash, source).position(|lock_hash| &lock_hash == owner_lock_hash)
}

pub fn build_l2_sudt_script(
    rollup_script_hash: &H256,
    config: &RollupConfig,
    l1_sudt_script_hash: &H256,
) -> Option<Script> {
    if l1_sudt_script_hash == &CKB_SUDT_SCRIPT_ARGS.into() {
        return None;
    }
    let args = {
        let mut args = Vec::with_capacity(64);
        args.extend(rollup_script_hash.as_slice());
        args.extend(l1_sudt_script_hash.as_slice());
        Bytes::from(args)
    };
    Some(
        Script::new_builder()
            .args(args.pack())
            .code_hash(config.l2_sudt_validator_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .build(),
    )
}
