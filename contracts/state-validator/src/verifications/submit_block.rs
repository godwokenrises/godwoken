// Import from `core` instead of from `std` since we are in no-std mode
use core::result::Result;

// Import heap related library from `alloc`
// https://doc.rust-lang.org/alloc/index.html
use alloc::{collections::BTreeMap, vec, vec::Vec};
use validator_utils::signature::check_input_account_lock;

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use crate::{
    cells::{
        build_l2_sudt_script, collect_custodian_locks, collect_deposition_locks,
        collect_withdrawal_locks, find_challenge_cell, find_one_stake_cell,
    },
    ckb_std::ckb_constants::Source,
    types::WithdrawalCell,
};

use super::check_status;
use crate::types::BlockContext;
use validator_utils::error::Error;

use gw_common::{
    builtins::CKB_SUDT_ACCOUNT_ID,
    error::Error as StateError,
    merkle_utils::calculate_merkle_root,
    smt::{Blake2bHasher, CompiledMerkleProof},
    state::State,
    CKB_SUDT_SCRIPT_ARGS, FINALIZE_BLOCKS, H256,
};
use gw_types::{
    bytes::Bytes,
    core::Status,
    packed::{
        AccountMerkleState, GlobalState, L2Block, RawL2Block, RollupConfig, WithdrawalRequest,
    },
    prelude::*,
};

fn check_withdrawal_cells(
    context: &mut BlockContext,
    mut withdrawal_requests: Vec<WithdrawalRequest>,
    withdrawal_cells: &[WithdrawalCell],
) -> Result<(), Error> {
    // iter outputs withdrawal cells, check each cell has a corresponded withdrawal request
    for cell in withdrawal_cells {
        // check withdrawal cell block info
        let withdrawal_block_hash: [u8; 32] = cell.args.withdrawal_block_hash().unpack();
        if withdrawal_block_hash != context.block_hash
            || cell.args.withdrawal_block_number().unpack() != context.number
        {
            return Err(Error::InvalidWithdrawalCell);
        }

        let cell_account_script_hash: H256 = cell.args.account_script_hash().unpack();
        // check that there is a corresponded withdrawal request
        let withdrawal_request = match withdrawal_requests.iter().position(|request| {
            let raw = request.raw();
            let account_script_hash: H256 = raw.account_script_hash().unpack();
            let sudt_script_hash: H256 = raw.sudt_script_hash().unpack();
            let amount: u128 = raw.amount().unpack();
            let capacity: u64 = raw.capacity().unpack();

            account_script_hash == cell_account_script_hash
                && sudt_script_hash == cell.value.sudt_script_hash
                && amount == cell.value.amount
                && capacity == cell.value.capacity
        }) {
            Some(index) => withdrawal_requests.remove(index),
            None => {
                return Err(Error::InvalidWithdrawalCell);
            }
        };
        // check that there is an input to unlock account
        let message = withdrawal_request.raw().hash().into();
        check_input_account_lock(cell_account_script_hash.into(), message)
            .map_err(|_| Error::InvalidWithdrawalCell)?;
    }
    // Some withdrawal requests hasn't has a corresponded withdrawal cell
    if !withdrawal_requests.is_empty() {
        return Err(Error::InvalidWithdrawalCell);
    }
    Ok(())
}

fn check_input_custodian_cells(
    config: &RollupConfig,
    context: &mut BlockContext,
    output_withdrawal_cells: Vec<WithdrawalCell>,
) -> Result<(), Error> {
    // collect input custodian cells
    let (finalized_custodian_cells, unfinalized_custodian_cells): (Vec<_>, Vec<_>) =
        collect_custodian_locks(&context.rollup_type_hash, config, Source::Input)?
            .into_iter()
            .partition(|cell| {
                let number: u64 = cell.args.deposition_block_number().unpack();
                number <= context.finalized_number
            });
    // check finalized custodian cells == withdrawal cells
    // we only need to verify the assets is equal
    {
        let mut withdrawal_assets = BTreeMap::new();
        for withdrawal in output_withdrawal_cells {
            let sudt_balance = withdrawal_assets
                .entry(withdrawal.value.sudt_script_hash)
                .or_insert(0u128);
            *sudt_balance = sudt_balance
                .checked_add(withdrawal.value.amount)
                .ok_or(Error::AmountOverflow)?;
            let ckb_balance = withdrawal_assets
                .entry(CKB_SUDT_SCRIPT_ARGS.into())
                .or_insert(0u128);
            *ckb_balance = ckb_balance
                .checked_add(withdrawal.value.capacity.into())
                .ok_or(Error::AmountOverflow)?;
        }
        for cell in finalized_custodian_cells {
            let sudt_balance = withdrawal_assets
                .entry(cell.value.sudt_script_hash)
                .or_insert(0);
            *sudt_balance = sudt_balance
                .checked_sub(cell.value.amount)
                .ok_or(Error::AmountOverflow)?;
            let ckb_balance = withdrawal_assets
                .entry(CKB_SUDT_SCRIPT_ARGS.into())
                .or_insert(0);
            *ckb_balance = ckb_balance
                .checked_sub(cell.value.capacity.into())
                .ok_or(Error::AmountOverflow)?;
        }
        // failed to check the equality
        if !withdrawal_assets.values().all(|&v| v == 0) {
            return Err(Error::InvalidWithdrawalCell);
        }
    }

    // check unfinalized custodian cells == reverted deposition requests
    let mut reverted_deposition_cells =
        collect_deposition_locks(&context.rollup_type_hash, config, Source::Output)?;
    for custodian_cell in unfinalized_custodian_cells {
        let index = reverted_deposition_cells
            .iter()
            .position(|cell| {
                custodian_cell.args.deposition_lock_args() == cell.args
                    && custodian_cell.value == cell.value
            })
            .ok_or(Error::InvalidWithdrawalCell)?;
        reverted_deposition_cells.remove(index);
    }
    if !reverted_deposition_cells.is_empty() {
        return Err(Error::InvalidWithdrawalCell);
    }
    Ok(())
}

fn check_output_custodian_cells(
    config: &RollupConfig,
    context: &mut BlockContext,
) -> Result<(), Error> {
    // collect output custodian cells
    let (finalized_custodian_cells, unfinalized_custodian_cells): (Vec<_>, Vec<_>) =
        collect_custodian_locks(&context.rollup_type_hash, config, Source::Output)?
            .into_iter()
            .partition(|cell| {
                let number: u64 = cell.args.deposition_block_number().unpack();
                number <= context.finalized_number
            });
    // check depositions request cells == unfinalized custodian cells
    let mut deposition_cells =
        collect_deposition_locks(&context.rollup_type_hash, config, Source::Output)?;
    for custodian_cell in unfinalized_custodian_cells {
        let index = deposition_cells
            .iter()
            .position(|cell| {
                custodian_cell.args.deposition_lock_args() == cell.args
                    && custodian_cell.value == cell.value
            })
            .ok_or(Error::InvalidCustodianCell)?;
        deposition_cells.remove(index);
    }
    if !deposition_cells.is_empty() {
        return Err(Error::InvalidDepositCell);
    }
    // check reverted withdrawals == finalized custodian cells
    {
        let reverted_withdrawals =
            collect_withdrawal_locks(&context.rollup_type_hash, config, Source::Input)?;
        let mut reverted_withdrawal_assets = BTreeMap::new();
        for cell in reverted_withdrawals {
            let sudt_balance = reverted_withdrawal_assets
                .entry(cell.value.sudt_script_hash)
                .or_insert(0u128);
            *sudt_balance = sudt_balance
                .checked_add(cell.value.amount)
                .ok_or(Error::AmountOverflow)?;
            let ckb_balance = reverted_withdrawal_assets
                .entry(CKB_SUDT_SCRIPT_ARGS.into())
                .or_insert(0u128);
            *ckb_balance = ckb_balance
                .checked_add(cell.value.capacity.into())
                .ok_or(Error::AmountOverflow)?;
        }
        for cell in finalized_custodian_cells {
            let sudt_balance = reverted_withdrawal_assets
                .entry(cell.value.sudt_script_hash)
                .or_insert(0);
            *sudt_balance = sudt_balance
                .checked_sub(cell.value.amount)
                .ok_or(Error::AmountOverflow)?;
            let ckb_balance = reverted_withdrawal_assets
                .entry(CKB_SUDT_SCRIPT_ARGS.into())
                .or_insert(0);
            *ckb_balance = ckb_balance
                .checked_sub(cell.value.capacity.into())
                .ok_or(Error::AmountOverflow)?;
        }
        // check the equality
        if !reverted_withdrawal_assets.values().all(|&v| v == 0) {
            return Err(Error::InvalidWithdrawalCell);
        }
    }
    Ok(())
}

fn mint_layer2_sudt(config: &RollupConfig, context: &mut BlockContext) -> Result<(), Error> {
    let deposition_requests =
        collect_deposition_locks(&context.rollup_type_hash, config, Source::Input)?;
    for request in &deposition_requests {
        // find or create user account
        let id = match context.get_account_id_by_script_hash(&request.account_script_hash.into())? {
            Some(id) => id,
            None => context.create_account(request.account_script_hash)?,
        };
        // mint CKB
        context.mint_sudt(CKB_SUDT_ACCOUNT_ID, id, request.value.capacity.into())?;
        if request.value.sudt_script_hash.as_slice() == &CKB_SUDT_SCRIPT_ARGS {
            if request.value.amount != 0 {
                // SUDT amount must equals to zero if sudt script hash is equals to CKB_SUDT_SCRIPT_ARGS
                return Err(Error::InvalidDepositCell);
            }
            continue;
        }
        // find or create Simple UDT account
        let l2_sudt_script = build_l2_sudt_script(config, request.value.sudt_script_hash.into());
        let l2_sudt_script_hash: [u8; 32] = l2_sudt_script.hash();
        let sudt_id = match context.get_account_id_by_script_hash(&l2_sudt_script_hash.into())? {
            Some(id) => id,
            None => context.create_account(l2_sudt_script_hash.into())?,
        };
        // prevent fake CKB SUDT, the caller should filter these invalid depositions
        if sudt_id == CKB_SUDT_ACCOUNT_ID {
            return Err(Error::InvalidDepositCell);
        }
        // mint SUDT
        context.mint_sudt(sudt_id, id, request.value.amount)?;
    }

    Ok(())
}

fn burn_layer2_sudt(
    config: &RollupConfig,
    context: &mut BlockContext,
    withdrawal_cells: &[WithdrawalCell],
) -> Result<(), Error> {
    for request in withdrawal_cells {
        let l2_sudt_script_hash: [u8; 32] =
            build_l2_sudt_script(config, request.value.sudt_script_hash.into()).hash();
        // find user account
        let id = context
            .get_account_id_by_script_hash(&request.args.account_script_hash().unpack())?
            .ok_or(StateError::MissingKey)?;
        // burn CKB
        context.burn_sudt(CKB_SUDT_ACCOUNT_ID, id, request.value.capacity.into())?;
        // find Simple UDT account
        let sudt_id = context
            .get_account_id_by_script_hash(&l2_sudt_script_hash.into())?
            .ok_or(StateError::MissingKey)?;
        // burn sudt
        context.burn_sudt(sudt_id, id, request.value.amount)?;
    }

    Ok(())
}

fn load_l2block_context(
    l2block: &L2Block,
    rollup_type_hash: [u8; 32],
    prev_global_state: &GlobalState,
    post_global_state: &GlobalState,
) -> Result<BlockContext, Error> {
    let raw_block = l2block.raw();

    // Check pre block merkle proof
    let number: u64 = raw_block.number().unpack();
    if number != prev_global_state.block().count().unpack() {
        return Err(Error::InvalidBlock);
    }

    // verify parent block hash
    if raw_block.parent_block_hash() != prev_global_state.tip_block_hash() {
        return Err(Error::InvalidBlock);
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
        return Err(Error::InvalidBlock);
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
        return Err(Error::InvalidBlock);
    }

    // Check post account state
    // Note: Because of the optimistic mechanism, we do not need to verify post account merkle root
    if raw_block.post_account().as_slice() != post_global_state.account().as_slice() {
        return Err(Error::InvalidPostGlobalState);
    }

    // Generate context
    let account_count: u32 = prev_global_state.account().count().unpack();
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

fn verify_block_producer(
    config: &RollupConfig,
    context: &BlockContext,
    block: &L2Block,
) -> Result<(), Error> {
    let raw_block = block.raw();
    let owner_lock_hash = raw_block.stake_cell_owner_lock_hash();
    let stake_cell = find_one_stake_cell(
        &context.rollup_type_hash,
        config,
        Source::Input,
        &owner_lock_hash,
    )?;
    // check stake cell capacity
    if stake_cell.value.capacity < config.required_staking_capacity().unpack() {
        return Err(Error::InvalidStakeCell);
    }
    // expected output stake args
    let expected_stake_lock_args = stake_cell
        .args
        .as_builder()
        .stake_block_number(raw_block.number())
        .build();
    let output_stake_cell = find_one_stake_cell(
        &context.rollup_type_hash,
        config,
        Source::Output,
        &owner_lock_hash,
    )?;
    if expected_stake_lock_args != output_stake_cell.args
        || stake_cell.value != output_stake_cell.value
    {
        return Err(Error::InvalidStakeCell);
    }

    Ok(())
}

fn check_block_transactions(_context: &mut BlockContext, block: &L2Block) -> Result<(), Error> {
    // check tx_witness_root
    let submit_transactions = block.raw().submit_transactions();
    let tx_witness_root: [u8; 32] = submit_transactions.tx_witness_root().unpack();
    let tx_count: u32 = submit_transactions.tx_count().unpack();
    let compacted_post_root_list = submit_transactions.compacted_post_root_list();

    if tx_count != compacted_post_root_list.item_count() as u32 {
        return Err(Error::InvalidTxsState);
    }

    let leaves = block
        .transactions()
        .into_iter()
        .map(|tx| tx.hash())
        .collect();
    let merkle_root: [u8; 32] = calculate_merkle_root(leaves)?;
    if tx_witness_root != merkle_root {
        return Err(Error::MerkleProof);
    }

    Ok(())
}

/// Verify Deposition & Withdrawal
pub fn verify(
    rollup_type_hash: [u8; 32],
    config: &RollupConfig,
    block: &L2Block,
    prev_global_state: &GlobalState,
    post_global_state: &GlobalState,
) -> Result<(), Error> {
    check_status(&prev_global_state, Status::Running)?;
    let mut context = load_l2block_context(
        block,
        rollup_type_hash,
        prev_global_state,
        post_global_state,
    )?;
    // Verify block producer
    verify_block_producer(config, &context, block)?;
    // Mint token: deposition requests -> layer2 SUDT
    mint_layer2_sudt(config, &mut context)?;
    // build withdrawl_cells
    let withdrawal_cells: Vec<_> =
        collect_withdrawal_locks(&context.rollup_type_hash, config, Source::Output)?;
    // Withdrawal token: Layer2 SUDT -> withdrawals
    burn_layer2_sudt(config, &mut context, &withdrawal_cells)?;
    // Check new cells and reverted cells: deposition / withdrawal / custodian
    let withdrawal_requests = block.withdrawal_requests().into_iter().collect();
    check_withdrawal_cells(&mut context, withdrawal_requests, &withdrawal_cells)?;
    check_input_custodian_cells(config, &mut context, withdrawal_cells)?;
    check_output_custodian_cells(config, &mut context)?;
    // Ensure no challenge cells in submitting block transaction
    if find_challenge_cell(&rollup_type_hash, config, Source::Input)?.is_some()
        || find_challenge_cell(&rollup_type_hash, config, Source::Output)?.is_some()
    {
        return Err(Error::InvalidChallengeCell);
    }
    // Check transactions
    check_block_transactions(&mut context, block)?;

    // Verify Post state
    let actual_post_global_state = {
        let root = context.calculate_root()?;
        let count = context.get_account_count()?;
        // calculate new account merkle state from block_context
        let account_merkle_state = AccountMerkleState::new_builder()
            .merkle_root(root.pack())
            .count(count.pack())
            .build();
        // we have verified the post block merkle state
        let block_merkle_state = post_global_state.block();
        // last finalized block number
        let last_finalized_block_number = context.finalized_number;

        prev_global_state
            .clone()
            .as_builder()
            .account(account_merkle_state)
            .block(block_merkle_state)
            .tip_block_hash(context.block_hash.pack())
            .last_finalized_block_number(last_finalized_block_number.pack())
            .build()
    };

    if &actual_post_global_state != post_global_state {
        return Err(Error::InvalidPostGlobalState);
    }

    Ok(())
}
