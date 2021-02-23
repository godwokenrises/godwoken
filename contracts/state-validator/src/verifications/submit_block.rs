// Import from `core` instead of from `std` since we are in no-std mode
use core::result::Result;

// Import heap related library from `alloc`
// https://doc.rust-lang.org/alloc/index.html
use alloc::{collections::BTreeMap, vec, vec::Vec};

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use crate::{
    cells::{
        build_l2_sudt_script, collect_custodian_locks, collect_deposition_locks,
        collect_withdrawal_locks, find_challenge_cell, find_one_stake_cell,
    },
    ckb_std::ckb_constants::Source,
    types::{CellValue, DepositionRequestCell, WithdrawalCell},
};

use super::check_status;
use crate::types::BlockContext;
use validator_utils::error::Error;

use gw_common::{
    builtins::CKB_SUDT_ACCOUNT_ID,
    error::Error as StateError,
    h256_ext::H256Ext,
    merkle_utils::{calculate_compacted_account_root, calculate_merkle_root},
    smt::{Blake2bHasher, CompiledMerkleProof},
    state::State,
    CKB_SUDT_SCRIPT_ARGS, H256,
};
use gw_types::{
    bytes::Bytes,
    core::{ScriptHashType, Status},
    packed::{
        AccountMerkleState, Byte32, GlobalState, L2Block, RawL2Block, RollupConfig,
        WithdrawalRequest,
    },
    prelude::*,
};

fn build_assets_map_from_cells<'a, I: Iterator<Item = &'a CellValue>>(
    cells: I,
) -> Result<BTreeMap<H256, u128>, Error> {
    let mut assets = BTreeMap::new();
    for cell in cells {
        let sudt_balance = assets.entry(cell.sudt_script_hash).or_insert(0u128);
        *sudt_balance = sudt_balance
            .checked_add(cell.amount)
            .ok_or(Error::AmountOverflow)?;
        let ckb_balance = assets.entry(CKB_SUDT_SCRIPT_ARGS.into()).or_insert(0u128);
        *ckb_balance = ckb_balance
            .checked_add(cell.capacity.into())
            .ok_or(Error::AmountOverflow)?;
    }
    Ok(assets)
}

fn check_withdrawal_cells(
    context: &BlockContext,
    mut withdrawal_requests: Vec<WithdrawalRequest>,
    withdrawal_cells: &[WithdrawalCell],
) -> Result<(), Error> {
    // iter outputs withdrawal cells, check each cell has a corresponded withdrawal request
    for cell in withdrawal_cells {
        // check withdrawal cell block info
        let withdrawal_block_hash: H256 = cell.args.withdrawal_block_hash().unpack();
        if withdrawal_block_hash != context.block_hash
            || cell.args.withdrawal_block_number().unpack() != context.number
        {
            return Err(Error::InvalidWithdrawalCell);
        }

        let cell_account_script_hash: H256 = cell.args.account_script_hash().unpack();
        // check that there is a corresponded withdrawal request
        match withdrawal_requests.iter().position(|request| {
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
            Some(index) => {
                withdrawal_requests.remove(index);
            }
            None => {
                return Err(Error::InvalidWithdrawalCell);
            }
        }
    }
    // Some withdrawal requests hasn't has a corresponded withdrawal cell
    if !withdrawal_requests.is_empty() {
        return Err(Error::InvalidWithdrawalCell);
    }
    Ok(())
}

fn check_input_custodian_cells(
    config: &RollupConfig,
    context: &BlockContext,
    output_withdrawal_cells: Vec<WithdrawalCell>,
) -> Result<BTreeMap<H256, u128>, Error> {
    // collect input custodian cells
    let (finalized_custodian_cells, unfinalized_custodian_cells): (Vec<_>, Vec<_>) =
        collect_custodian_locks(&context.rollup_type_hash, config, Source::Input)?
            .into_iter()
            .partition(|cell| {
                let number: u64 = cell.args.deposition_block_number().unpack();
                number <= context.finalized_number
            });
    // check unfinalized custodian cells == reverted deposition requests
    let mut reverted_deposit_cells =
        collect_deposition_locks(&context.rollup_type_hash, config, Source::Output)?;
    for custodian_cell in unfinalized_custodian_cells {
        let index = reverted_deposit_cells
            .iter()
            .position(|cell| {
                custodian_cell.args.deposition_lock_args() == cell.args
                    && custodian_cell.value == cell.value
            })
            .ok_or(Error::InvalidWithdrawalCell)?;
        reverted_deposit_cells.remove(index);
    }
    if !reverted_deposit_cells.is_empty() {
        return Err(Error::InvalidWithdrawalCell);
    }
    // check input finalized custodian cells >= withdrawal cells
    let withdrawal_assets =
        build_assets_map_from_cells(output_withdrawal_cells.iter().map(|c| &c.value))?;
    let mut input_finalized_assets =
        build_assets_map_from_cells(finalized_custodian_cells.iter().map(|c| &c.value))?;
    // calculate input finalized custodian assets - withdrawal assets
    for (k, v) in withdrawal_assets {
        let balance = input_finalized_assets.entry(k).or_insert(0);
        *balance = balance
            .checked_sub(v)
            .ok_or(Error::InsufficientInputFinalizedAssets)?;
    }
    Ok(input_finalized_assets)
}

fn check_output_custodian_cells(
    config: &RollupConfig,
    context: &BlockContext,
    mut deposit_cells: Vec<DepositionRequestCell>,
    input_finalized_assets: BTreeMap<H256, u128>,
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
    for custodian_cell in unfinalized_custodian_cells {
        let index = deposit_cells
            .iter()
            .position(|cell| {
                custodian_cell.args.deposition_lock_args() == cell.args
                    && custodian_cell.value == cell.value
            })
            .ok_or(Error::InvalidCustodianCell)?;
        deposit_cells.remove(index);
    }
    if !deposit_cells.is_empty() {
        return Err(Error::InvalidDepositCell);
    }
    // check reverted withdrawals <= finalized custodian cells
    {
        let reverted_withdrawals =
            collect_withdrawal_locks(&context.rollup_type_hash, config, Source::Input)?;
        let reverted_withdrawal_assets =
            build_assets_map_from_cells(reverted_withdrawals.iter().map(|c| &c.value))?;
        let mut output_finalized_assets =
            build_assets_map_from_cells(finalized_custodian_cells.iter().map(|c| &c.value))?;
        // calculate output finalized assets - reverted withdrawal assets
        for (k, v) in reverted_withdrawal_assets {
            let balance = output_finalized_assets.entry(k).or_insert(0);
            *balance = balance
                .checked_sub(v)
                .ok_or(Error::InsufficientOutputFinalizedAssets)?;
        }
        // check the remain inputs finalized assets == outputs finalized assets
        // 1. output finalized assets - input finalized assets
        for (k, v) in input_finalized_assets {
            let balance = output_finalized_assets.entry(k).or_insert(0);
            *balance = balance
                .checked_sub(v)
                .ok_or(Error::InsufficientOutputFinalizedAssets)?;
        }
        // 2. check output finalized assets is empty
        let output_assets_is_empty = output_finalized_assets.iter().all(|(_k, v)| v == &0);
        if !output_assets_is_empty {
            return Err(Error::InsufficientInputFinalizedAssets);
        }
    }
    Ok(())
}

fn mint_layer2_sudt(
    config: &RollupConfig,
    context: &mut BlockContext,
    deposit_cells: &[DepositionRequestCell],
) -> Result<(), Error> {
    for request in deposit_cells {
        // check that account's script is a valid EOA script
        if request.account_script.hash_type() != ScriptHashType::Type.into() {
            return Err(Error::UnknownEOAScript);
        }
        if config
            .allowed_eoa_type_hashes()
            .into_iter()
            .find(|code_hash| code_hash == &request.account_script.code_hash())
            .is_none()
        {
            return Err(Error::UnknownEOAScript);
        }
        // find or create EOA
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
    block: &L2Block,
) -> Result<(), Error> {
    for request in block.withdrawals() {
        let raw = request.raw();
        let l2_sudt_script_hash: [u8; 32] =
            build_l2_sudt_script(config, raw.sudt_script_hash().unpack()).hash();
        // find EOA
        let id = context
            .get_account_id_by_script_hash(&raw.account_script_hash().unpack())?
            .ok_or(StateError::MissingKey)?;
        // burn CKB
        context.burn_sudt(CKB_SUDT_ACCOUNT_ID, id, raw.capacity().unpack() as u128)?;
        // find Simple UDT account
        let sudt_id = context
            .get_account_id_by_script_hash(&l2_sudt_script_hash.into())?
            .ok_or(StateError::MissingKey)?;
        // burn sudt
        context.burn_sudt(sudt_id, id, raw.amount().unpack())?;
        // update nonce
        let nonce = context.get_nonce(id)?;
        let withdrawal_nonce: u32 = raw.nonce().unpack();
        if nonce != withdrawal_nonce {
            return Err(Error::InvalidWithdrawalRequest);
        }
        context.set_nonce(id, nonce.saturating_add(1))?;
    }

    Ok(())
}

fn load_l2block_context(
    rollup_type_hash: H256,
    config: &RollupConfig,
    l2block: &L2Block,
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
    let block_hash: H256 = raw_block.hash().into();
    if !block_merkle_proof
        .verify::<Blake2bHasher>(
            &post_block_root.into(),
            vec![(block_smt_key.into(), block_hash.clone())],
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
    let prev_account_root = prev_global_state.account().merkle_root().unpack();
    let finalized_number = number.saturating_sub(config.finality_blocks().unpack());
    let context = BlockContext {
        number,
        finalized_number,
        kv_pairs,
        kv_merkle_proof,
        account_count,
        rollup_type_hash,
        block_hash,
        prev_account_root,
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

fn check_block_transactions(context: &BlockContext, block: &L2Block) -> Result<(), Error> {
    // check tx_witness_root
    let raw_block = block.raw();
    let submit_transactions = raw_block.submit_transactions();
    let tx_witness_root: [u8; 32] = submit_transactions.tx_witness_root().unpack();
    let tx_count: u32 = submit_transactions.tx_count().unpack();
    let compacted_post_root_list = submit_transactions.compacted_post_root_list();

    if tx_count != compacted_post_root_list.item_count() as u32
        || tx_count != block.transactions().len() as u32
    {
        return Err(Error::InvalidTxsState);
    }

    let leaves = block
        .transactions()
        .into_iter()
        .map(|tx| tx.witness_hash())
        .collect();
    let merkle_root: [u8; 32] = calculate_merkle_root(leaves)?;
    if tx_witness_root != merkle_root {
        return Err(Error::MerkleProof);
    }

    // check current account tree state
    let compacted_prev_root_hash: H256 = submit_transactions.compacted_prev_root_hash().unpack();
    if context.calculate_compacted_account_root()? != compacted_prev_root_hash {
        return Err(Error::InvalidTxsState);
    }

    // check post account tree state
    let post_compacted_account_root = submit_transactions
        .compacted_post_root_list()
        .into_iter()
        .last()
        .unwrap_or(submit_transactions.compacted_prev_root_hash());
    let block_post_compacted_account_root: Byte32 = {
        let account = raw_block.post_account();
        calculate_compacted_account_root(&account.merkle_root().unpack(), account.count().unpack())
            .pack()
    };
    if post_compacted_account_root != block_post_compacted_account_root {
        return Err(Error::InvalidTxsState);
    }

    Ok(())
}

fn check_block_withdrawals_root(block: &L2Block) -> Result<(), Error> {
    // check withdrawal_witness_root
    let submit_withdrawals = block.raw().submit_withdrawals();
    let withdrawal_witness_root: [u8; 32] = submit_withdrawals.withdrawal_witness_root().unpack();
    let withdrawal_count: u32 = submit_withdrawals.withdrawal_count().unpack();

    if withdrawal_count != block.withdrawals().len() as u32 {
        return Err(Error::InvalidBlock);
    }

    let leaves = block
        .withdrawals()
        .into_iter()
        .map(|withdrawal| withdrawal.witness_hash())
        .collect();
    let merkle_root: [u8; 32] = calculate_merkle_root(leaves)?;
    if withdrawal_witness_root != merkle_root {
        return Err(Error::MerkleProof);
    }

    Ok(())
}

/// Verify Deposition & Withdrawal
pub fn verify(
    rollup_type_hash: H256,
    config: &RollupConfig,
    block: &L2Block,
    prev_global_state: &GlobalState,
    post_global_state: &GlobalState,
) -> Result<(), Error> {
    check_status(&prev_global_state, Status::Running)?;
    // Check withdrawals root
    check_block_withdrawals_root(block)?;
    let mut context = load_l2block_context(
        rollup_type_hash,
        config,
        block,
        prev_global_state,
        post_global_state,
    )?;
    // Verify block producer
    verify_block_producer(config, &context, block)?;
    // collect withdrawal cells
    let withdrawal_cells: Vec<_> =
        collect_withdrawal_locks(&context.rollup_type_hash, config, Source::Output)?;
    // collect deposit cells
    let deposit_cells = collect_deposition_locks(&context.rollup_type_hash, config, Source::Input)?;
    // Check new cells and reverted cells: deposition / withdrawal / custodian
    let withdrawal_requests = block.withdrawals().into_iter().collect();
    check_withdrawal_cells(&context, withdrawal_requests, &withdrawal_cells)?;
    let input_finalized_assets = check_input_custodian_cells(config, &context, withdrawal_cells)?;
    check_output_custodian_cells(
        config,
        &context,
        deposit_cells.clone(),
        input_finalized_assets,
    )?;
    // Ensure no challenge cells in submitting block transaction
    if find_challenge_cell(&rollup_type_hash, config, Source::Input)?.is_some()
        || find_challenge_cell(&rollup_type_hash, config, Source::Output)?.is_some()
    {
        return Err(Error::InvalidChallengeCell);
    }

    // Withdrawal token: Layer2 SUDT -> withdrawals
    burn_layer2_sudt(config, &mut context, block)?;
    // Mint token: deposition requests -> layer2 SUDT
    mint_layer2_sudt(config, &mut context, &deposit_cells)?;
    // Check transactions
    check_block_transactions(&context, block)?;

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

// Verify reverted_block_root
pub fn verify_reverted_block_hashes(
    reverted_block_hashes: Vec<H256>,
    reverted_block_proof: Bytes,
    prev_global_state: &GlobalState,
) -> Result<(), Error> {
    let reverted_block_root = prev_global_state.reverted_block_root().unpack();
    let merkle_proof = CompiledMerkleProof(reverted_block_proof.into());
    let leaves: Vec<_> = reverted_block_hashes
        .into_iter()
        .map(|k| (k, H256::one()))
        .collect();
    if leaves.is_empty() && merkle_proof.0.is_empty() {
        return Ok(());
    }
    let valid = merkle_proof.verify::<Blake2bHasher>(&reverted_block_root, leaves)?;
    if !valid {
        return Err(Error::MerkleProof);
    }
    Ok(())
}
