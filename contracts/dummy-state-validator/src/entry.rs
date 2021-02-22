// Import from `core` instead of from `std` since we are in no-std mode
use core::result::Result;

// Import heap related library from `alloc`
// https://doc.rust-lang.org/alloc/index.html
use alloc::{collections::BTreeMap, vec};
use validator_utils::ckb_std::high_level::load_cell_capacity;

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use crate::ckb_std::{
    ckb_constants::Source,
    ckb_types::{bytes::Bytes, prelude::*},
    high_level::{load_cell_data, load_script_hash, load_witness_args},
};

use gw_types::{
    packed::{GlobalState, GlobalStateReader, L2Block, L2BlockReader, RawL2Block},
    prelude::{Reader as GodwokenTypesReader, Unpack as GodwokenTypesUnpack},
};

use gw_common::{
    smt::Blake2bHasher,
    sparse_merkle_tree::{CompiledMerkleProof, H256},
};

use crate::context::Context;
use crate::error::Error;

fn parse_l2block() -> Result<L2Block, Error> {
    let witness_args = load_witness_args(0, Source::GroupOutput)?;
    let output_type: Bytes = witness_args
        .output_type()
        .to_opt()
        .ok_or_else(|| Error::Encoding)?
        .unpack();
    match L2BlockReader::verify(&output_type, false) {
        Ok(_) => Ok(L2Block::new_unchecked(output_type)),
        Err(_) => Err(Error::Encoding),
    }
}

fn parse_global_state(source: Source) -> Result<GlobalState, Error> {
    let data = load_cell_data(0, source)?;
    match GlobalStateReader::verify(&data, false) {
        Ok(_) => Ok(GlobalState::new_unchecked(data.into())),
        Err(_) => Err(Error::Encoding),
    }
}

fn verify_l2block(
    l2block: &L2Block,
    prev_global_state: &GlobalState,
    post_global_state: &GlobalState,
) -> Result<Context, Error> {
    let raw_block = l2block.raw();

    // Check pre block merkle proof
    let number: u64 = raw_block.number().unpack();
    if number != prev_global_state.block().count().unpack() {
        return Err(Error::PrevGlobalState);
    }

    let block_smt_key: H256 = RawL2Block::compute_smt_key(number).into();
    let block_proof: Bytes = l2block.block_proof().unpack();
    let block_merkle_proof = CompiledMerkleProof(block_proof.to_vec());
    let prev_block_root: [u8; 32] = prev_global_state.block().merkle_root().unpack();
    if !block_merkle_proof
        .verify::<Blake2bHasher>(&prev_block_root.into(), vec![(block_smt_key, H256::zero())])
        .map_err(|_| Error::MerkleProof)?
    {
        return Err(Error::MerkleProof);
    }

    // Check post block merkle proof
    if number + 1 != post_global_state.block().count().unpack() {
        return Err(Error::PrevGlobalState);
    }

    let post_block_root: [u8; 32] = post_global_state.block().merkle_root().unpack();
    let block_hash = raw_block.hash();
    if !block_merkle_proof
        .verify::<Blake2bHasher>(
            &post_block_root.into(),
            vec![(block_smt_key, block_hash.into())],
        )
        .map_err(|_| Error::MerkleProof)?
    {
        return Err(Error::MerkleProof);
    }

    // Check pre account merkle proof
    let kv_state_proof: Bytes = l2block.kv_state_proof().unpack();
    let kv_merkle_proof = CompiledMerkleProof(kv_state_proof.to_vec());
    let kv_pairs: BTreeMap<_, _> = l2block
        .kv_state()
        .into_iter()
        .map(|kv| {
            let k: [u8; 32] = kv.k().unpack();
            let v: [u8; 32] = kv.v().unpack();
            (k.into(), v.into())
        })
        .collect();
    let prev_account_root: [u8; 32] = prev_global_state.account().merkle_root().unpack();
    let is_blank_kv = kv_merkle_proof.0.len() == 0 && kv_pairs.is_empty();
    if !is_blank_kv
        && !kv_merkle_proof
            .verify::<Blake2bHasher>(
                &prev_account_root.into(),
                kv_pairs.iter().map(|(k, v)| (*k, *v)).collect(),
            )
            .map_err(|_| Error::MerkleProof)?
    {
        return Err(Error::MerkleProof);
    }

    // Check prev account state
    if raw_block.prev_account().as_slice() != prev_global_state.account().as_slice() {
        return Err(Error::PrevGlobalState);
    }

    // Check post account state
    // Note: Because of the optimistic mechanism, we do not need to verify post account merkle root
    if raw_block.post_account().as_slice() != post_global_state.account().as_slice() {
        return Err(Error::PostGlobalState);
    }

    // Generate context
    let account_count: u32 = prev_global_state.account().count().unpack();
    let rollup_type_hash = load_script_hash()?;
    let aggregator_id: u32 = raw_block.block_producer_id().unpack();
    let context = Context {
        number,
        aggregator_id,
        kv_pairs,
        kv_merkle_proof,
        account_count,
        rollup_type_hash,
        block_hash,
    };

    Ok(context)
}

pub fn main() -> Result<(), Error> {
    // no input Rollup cell, which represents we are on the initialization
    if load_cell_capacity(0, Source::GroupInput).is_err() {
        return Ok(());
    }
    // otherwise, check the global state and l2block
    let prev_global_state = parse_global_state(Source::GroupInput)?;
    let post_global_state = parse_global_state(Source::GroupOutput)?;
    let l2block = parse_l2block()?;
    verify_l2block(&l2block, &prev_global_state, &post_global_state)?;
    Ok(())
}
