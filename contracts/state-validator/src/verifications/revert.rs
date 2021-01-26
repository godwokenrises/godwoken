use gw_common::{
    h256_ext::H256Ext,
    smt::{Blake2bHasher, CompiledMerkleProof},
    FINALIZE_BLOCKS, H256,
};
use gw_types::{
    core::Status,
    packed::{
        BlockMerkleState, Byte32, ChallengeTarget, GlobalState, RawL2Block, RollupConfig,
        RollupRevert, Script,
    },
    prelude::*,
};
use validator_utils::{
    ckb_std::{
        ckb_constants::Source,
        high_level::load_input_since,
        since::{LockValue, Since},
    },
    search_cells::search_lock_hash,
};

use super::{check_rollup_lock_cells, check_status};
use crate::{
    cells::{
        collect_burn_cells, collect_custodian_locks, collect_deposition_locks,
        collect_withdrawal_locks, fetch_capacity_and_sudt_value, find_challenge_cell,
        find_stake_cell,
    },
    error::Error,
    types::{ChallengeCell, StakeCell},
};
use alloc::{vec, vec::Vec};
use core::{convert::TryInto, num};

/// Check challenge cell is maturity(on the layer1)
fn check_challenge_maturity(
    _config: &RollupConfig,
    challenge_cell: &ChallengeCell,
) -> Result<(), Error> {
    const CHALLENGE_MATURITY_BLOCKS: u64 = 10000;

    let since = Since::new(load_input_since(challenge_cell.index, Source::Input)?);
    match since.extract_lock_value() {
        Some(LockValue::BlockNumber(n)) => {
            if since.is_relative() && n > CHALLENGE_MATURITY_BLOCKS {
                return Ok(());
            }
        }

        _ => {}
    }
    Err(Error::InvalidStatus)
}

fn check_challenge_cell(
    rollup_type_hash: &[u8; 32],
    config: &RollupConfig,
    challenge_cell: &ChallengeCell,
    revert_target_block_hash: &H256,
) -> Result<(), Error> {
    // check challenge maturity
    check_challenge_maturity(config, challenge_cell)?;
    // check other challenge cells
    let has_output_challenge =
        find_challenge_cell(&rollup_type_hash, config, Source::Output)?.is_some();
    if has_output_challenge {
        return Err(Error::Challenge);
    }
    // check challenge target
    let challenge_target = challenge_cell.args.target();
    let challenge_block_hash: H256 = challenge_target.block_hash().unpack();
    if &challenge_block_hash != revert_target_block_hash {
        return Err(Error::Challenge);
    }
    // challenge cell should be send back to the challenger
    let receiver_cell_value = {
        let reward_receiver_lock_hash = challenge_cell.args.rewards_receiver_lock().hash();
        let index =
            search_lock_hash(&reward_receiver_lock_hash, Source::Output).ok_or(Error::Challenge)?;
        fetch_capacity_and_sudt_value(config, index, Source::Output)?
    };
    if receiver_cell_value.capacity < challenge_cell.value.capacity {
        return Err(Error::Challenge);
    }
    Ok(())
}

fn check_reverted_blocks(
    reverted_blocks: &[RawL2Block],
    revert_args: &RollupRevert,
    prev_global_state: &GlobalState,
    post_global_state: &GlobalState,
) -> Result<GlobalState, Error> {
    if reverted_blocks.is_empty() {
        return Err(Error::Challenge);
    }
    let reverted_block_hashes: Vec<H256> =
        reverted_blocks.iter().map(|b| b.hash().into()).collect();
    let reverted_block_smt_keys: Vec<H256> = reverted_blocks
        .iter()
        .map(|b| RawL2Block::compute_smt_key(b.number().unpack()).into())
        .collect();
    // check reverted_blocks is continues
    {
        let mut prev_hash: Byte32 = reverted_blocks[0].hash().pack();
        let mut prev_number = reverted_blocks[0].number().unpack();
        for b in reverted_blocks[1..].iter() {
            let hash = b.parent_block_hash();
            if hash != prev_hash {
                return Err(Error::Challenge);
            }
            let number: u64 = b.number().unpack();
            if number != prev_number + 1 {
                return Err(Error::Challenge);
            }
            prev_hash = hash;
            prev_number = number;
        }

        // must revert from current point to the tip block
        let tip_number: u64 = prev_global_state.block().count().unpack();
        if prev_number != tip_number {
            return Err(Error::Challenge);
        }
    }
    // prove the target block exists in the main chain
    let block_merkle_proof = CompiledMerkleProof(revert_args.block_proof().unpack());
    let is_main_chain_block = {
        let leaves = reverted_block_smt_keys
            .clone()
            .into_iter()
            .zip(reverted_block_hashes.clone())
            .collect();
        block_merkle_proof
            .verify::<Blake2bHasher>(&prev_global_state.block().merkle_root().unpack(), leaves)?
    };
    if !is_main_chain_block {
        return Err(Error::Challenge);
    }
    // prove the target block isn't in the prev reverted block root
    let reverted_block_merkle_proof =
        CompiledMerkleProof(revert_args.reverted_block_proof().unpack());
    let is_reverted_block_prev = {
        let leaves = reverted_block_hashes
            .clone()
            .into_iter()
            .map(|hash| (hash, H256::zero()))
            .collect();
        reverted_block_merkle_proof
            .verify::<Blake2bHasher>(&prev_global_state.reverted_block_root().unpack(), leaves)?
    };
    if is_reverted_block_prev {
        return Err(Error::Challenge);
    }
    // prove the target block in the post reverted block root
    let is_reverted_block_post = {
        let leaves = reverted_block_hashes
            .clone()
            .into_iter()
            .map(|hash| (hash, H256::one()))
            .collect();
        reverted_block_merkle_proof
            .verify::<Blake2bHasher>(&post_global_state.reverted_block_root().unpack(), leaves)?
    };
    if !is_reverted_block_post {
        return Err(Error::Challenge);
    }
    let reverted_block_root = post_global_state.reverted_block_root();
    // calculate the prev block merkle state (delete reverted block hashes)
    let block_merkle_state = {
        let leaves = reverted_block_smt_keys
            .clone()
            .into_iter()
            .map(|smt_key| (smt_key, H256::zero()))
            .collect();
        let block_root = block_merkle_proof.compute_root::<Blake2bHasher>(leaves)?;
        let block_count = reverted_blocks[0].number();
        BlockMerkleState::new_builder()
            .merkle_root(block_root.pack())
            .count(block_count)
            .build()
    };
    let account_merkle_state = reverted_blocks[0].prev_account();
    let last_finalized_block_number = {
        let number: u64 = reverted_blocks[0].number().unpack();
        number.saturating_sub(1).saturating_sub(FINALIZE_BLOCKS)
    };
    // check post global state
    let reverted_post_global_state = {
        let status: u8 = Status::Running.into();
        prev_global_state
            .clone()
            .as_builder()
            .account(account_merkle_state)
            .block(block_merkle_state)
            .last_finalized_block_number(last_finalized_block_number.pack())
            .reverted_block_root(reverted_block_root)
            .status(status.into())
            .build()
    };
    Ok(reverted_post_global_state)
}

/// Verify revert
/// 1. check revert merkle roots
/// 2. check reverted block root
/// 3. check other lock cells
pub fn verify(
    rollup_type_hash: [u8; 32],
    config: &RollupConfig,
    prev_global_state: &GlobalState,
    post_global_state: &GlobalState,
    revert_args: RollupRevert,
) -> Result<(), Error> {
    check_status(prev_global_state, Status::Halting)?;
    // check rollup lock cells,
    // we do not handle the reverting of lock cells in here,
    // instead we handle them in the submitting layer2 block action
    check_rollup_lock_cells(&rollup_type_hash, config)?;
    // load reverted blocks
    let reverted_blocks: Vec<_> = revert_args.reverted_blocks().into_iter().collect();
    // check challenge cells
    let challenge_cell =
        find_challenge_cell(&rollup_type_hash, config, Source::Input)?.ok_or(Error::Challenge)?;
    check_challenge_cell(
        &rollup_type_hash,
        config,
        &challenge_cell,
        &reverted_blocks[0].hash().into(),
    )?;
    let reverted_global_state = check_reverted_blocks(
        &reverted_blocks,
        &revert_args,
        prev_global_state,
        post_global_state,
    )?;
    if post_global_state != &reverted_global_state {
        return Err(Error::PostGlobalState);
    }
    Ok(())
}
