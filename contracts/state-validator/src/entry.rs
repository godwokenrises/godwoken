// Import from `core` instead of from `std` since we are in no-std mode
use core::mem::size_of_val;
use core::result::Result;

// Import heap related library from `alloc`
// https://doc.rust-lang.org/alloc/index.html
use alloc::{collections::BTreeMap, vec, vec::Vec};
use validator_utils::{
    ckb_std::high_level::{load_cell_capacity, load_cell_data_hash},
    search_cells::search_rollup_config_cell,
    signature::check_input_account_lock,
};

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use crate::{
    ckb_std::{
        ckb_constants::Source,
        ckb_types::{bytes::Bytes, prelude::*},
        dynamic_loading::CKBDLContext,
        high_level::{load_cell_data, load_script_hash, load_witness_args},
    },
    verifications,
};

use gw_types::{
    packed::{
        GlobalState, GlobalStateReader, L2Block, L2BlockReader, RawL2Block, RollupAction,
        RollupActionReader, RollupActionUnion, RollupConfig, RollupConfigReader,
    },
    prelude::{Reader as GodwokenTypesReader, Unpack as GodwokenTypesUnpack},
};

use gw_common::{
    blake2b::new_blake2b,
    smt::Blake2bHasher,
    sparse_merkle_tree::{CompiledMerkleProof, H256},
    state::State,
    FINALIZE_BLOCKS,
};

use crate::consensus::verify_block_producer;
use crate::error::Error;
use crate::types::BlockContext;

// TODO 1. consider contract on creation
// TODO 2. make sure we only have 1 contract cell
fn parse_rollup_action() -> Result<RollupAction, Error> {
    let witness_args = load_witness_args(0, Source::GroupOutput)?;
    let output_type: Bytes = witness_args
        .output_type()
        .to_opt()
        .ok_or_else(|| Error::Encoding)?
        .unpack();
    match RollupActionReader::verify(&output_type, false) {
        Ok(_) => Ok(RollupAction::new_unchecked(output_type)),
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

fn load_rollup_config(rollup_config_hash: &[u8; 32]) -> Result<RollupConfig, Error> {
    let index = search_rollup_config_cell(rollup_config_hash).ok_or(Error::IndexOutOfBound)?;
    let data = load_cell_data(index, Source::CellDep)?;
    match RollupConfigReader::verify(&data, false) {
        Ok(_) => Ok(RollupConfig::new_unchecked(data.into())),
        Err(_) => return Err(Error::Encoding),
    }
}

fn load_l2block_context(
    l2block: &L2Block,
    prev_global_state: &GlobalState,
    post_global_state: &GlobalState,
) -> Result<BlockContext, Error> {
    // TODO verify parent block hash
    let raw_block = l2block.raw();

    // Check pre block merkle proof
    let number: u64 = raw_block.number().unpack();
    if number != prev_global_state.block().count().unpack() {
        return Err(Error::PrevGlobalState);
    }

    let block_smt_key = RawL2Block::compute_smt_key(number);
    let block_proof: Bytes = l2block.block_proof().unpack();
    let block_merkle_proof = CompiledMerkleProof(block_proof.to_vec());
    let prev_block_root: [u8; 32] = prev_global_state.block().merkle_root().unpack();
    if !block_merkle_proof
        .verify::<Blake2bHasher>(
            &prev_block_root.into(),
            vec![(block_smt_key.into(), H256::zero())],
        )
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
            vec![(block_smt_key.into(), block_hash.into())],
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
    let finalized_number = number.saturating_sub(FINALIZE_BLOCKS);
    let context = BlockContext {
        number,
        finalized_number,
        kv_pairs,
        kv_merkle_proof,
        account_count,
        rollup_type_hash,
        block_hash,
    };

    Ok(context)
}

/// return true if we are in the initialization, otherwise return false
fn check_initialization() -> Result<bool, Error> {
    if load_cell_capacity(0, Source::GroupInput).is_ok() {
        return Ok(false);
    }
    // no input Rollup cell, which represents we are in the initialization
    let post_global_state = parse_global_state(Source::GroupOutput)?;
    // check config cell
    let _rollup_config = load_rollup_config(&post_global_state.rollup_config_hash().unpack())?;
    Ok(true)
}

pub fn main() -> Result<(), Error> {
    // return success if we are in the initialization
    if check_initialization()? {
        return Ok(());
    }
    // basic verification
    let prev_global_state = parse_global_state(Source::GroupInput)?;
    let post_global_state = parse_global_state(Source::GroupOutput)?;
    let action = parse_rollup_action()?;
    match action.to_enum() {
        RollupActionUnion::L2Block(l2block) => {
            let rollup_config =
                load_rollup_config(&prev_global_state.rollup_config_hash().unpack())?;
            let mut context =
                load_l2block_context(&l2block, &prev_global_state, &post_global_state)?;
            // Verify block producer
            verify_block_producer(&rollup_config, &context, &l2block)?;
            verifications::layer1_cells::verify(&rollup_config, &mut context, &l2block)?;

            // handle state transitions
            // verifications::submit_transactions::verify(&mut context, &l2block)?;
        }
        _ => {
            panic!("unknown rollup action");
        }
    }
    // TODO verify GlobalState

    Ok(())
}
