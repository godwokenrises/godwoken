// Import from `core` instead of from `std` since we are in no-std mode
use core::result::Result;

// Import heap related library from `alloc`
// https://doc.rust-lang.org/alloc/index.html
use alloc::{collections::BTreeMap, vec::Vec};
use gw_state::ckb_smt::smt::{Pair, Tree};
use gw_state::constants::GW_MAX_KV_PAIRS;
use gw_utils::ckb_std::high_level::load_input_since;
use gw_utils::ckb_std::since::{LockValue, Since};
use gw_utils::gw_common::registry_address::RegistryAddress;
use gw_utils::gw_types::packed::{L2BlockReader, WithdrawalRequestReader};

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use crate::ckb_std::{ckb_constants::Source, debug};
use gw_state::kv_state::KVState;
use gw_utils::finality::{finality_time_in_ms, is_finalized};
use gw_utils::gw_common::{self, ckb_decimal::CKBCapacity};
use gw_utils::gw_types::{self, U256};

use super::check_status;
use crate::types::BlockContext;
use gw_utils::{
    cells::{
        lock_cells::{
            collect_custodian_locks, collect_deposit_locks, collect_withdrawal_locks,
            find_block_producer_stake_cell, find_challenge_cell,
        },
        types::{CellValue, DepositRequestCell, WithdrawalCell},
        utils::build_l2_sudt_script,
    },
    error::Error,
    fork::Fork,
};

use gw_common::{
    builtins::CKB_SUDT_ACCOUNT_ID,
    error::Error as StateError,
    h256_ext::H256Ext,
    merkle_utils::{calculate_ckb_merkle_root, calculate_state_checkpoint, ckb_merkle_leaf_hash},
    state::State,
    CKB_SUDT_SCRIPT_ARGS, H256,
};
use gw_types::{
    bytes::Bytes,
    core::{ScriptHashType, Status, Timepoint},
    packed::{Byte32, GlobalState, RawL2Block, RollupConfig},
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

fn check_withdrawal_cells<'a>(
    context: &BlockContext,
    mut withdrawal_requests: Vec<WithdrawalRequestReader<'a>>,
    withdrawal_cells: &[WithdrawalCell],
) -> Result<(), Error> {
    // iter outputs withdrawal cells, check each cell has a corresponded withdrawal request
    for cell in withdrawal_cells {
        // check withdrawal cell block info
        let withdrawal_block_hash: H256 = cell.args.withdrawal_block_hash().unpack();
        if withdrawal_block_hash != context.block_hash {
            debug!("withdrawal cell mismatch block_hash");
            return Err(Error::InvalidWithdrawalCell);
        }

        if cell.args.withdrawal_block_timepoint().unpack() != context.block_timepoint().full_value()
        {
            debug!(
                "withdrawal_cell.args.withdrawal_block_timepoint != context.block_timepoint, {} != {}",
                cell.args.withdrawal_block_timepoint().unpack(),
                context.block_timepoint().full_value()
            );
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
                debug!("withdrawal cell mismatch the amount of assets");
                return Err(Error::InvalidWithdrawalCell);
            }
        }
    }
    // Some withdrawal requests hasn't has a corresponded withdrawal cell
    if !withdrawal_requests.is_empty() {
        debug!(
            "withdrawal requests has no corresponded withdrawal cells: {}",
            withdrawal_requests.len()
        );
        return Err(Error::InvalidWithdrawalCell);
    }
    Ok(())
}

fn check_input_custodian_cells(
    config: &RollupConfig,
    prev_global_state: &GlobalState,
    context: &BlockContext,
    output_withdrawal_cells: Vec<WithdrawalCell>,
) -> Result<BTreeMap<H256, u128>, Error> {
    // collect input custodian cells
    let (finalized_custodian_cells, unfinalized_custodian_cells): (Vec<_>, Vec<_>) =
        collect_custodian_locks(&context.rollup_type_hash, config, Source::Input)?
            .into_iter()
            .partition(|cell| {
                is_finalized(
                    config,
                    prev_global_state,
                    &Timepoint::from_full_value(cell.args.deposit_block_timepoint().unpack()),
                )
            });
    // check unfinalized custodian cells == reverted deposit requests
    let mut reverted_deposit_cells =
        collect_deposit_locks(&context.rollup_type_hash, config, Source::Output)?;
    for custodian_cell in unfinalized_custodian_cells {
        let index = reverted_deposit_cells
            .iter()
            .position(|cell| {
                custodian_cell.args.deposit_lock_args() == cell.args
                    && custodian_cell.value == cell.value
            })
            .ok_or(Error::InvalidCustodianCell)?;
        reverted_deposit_cells.remove(index);
    }
    if !reverted_deposit_cells.is_empty() {
        return Err(Error::InvalidDepositCell);
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
    prev_global_state: &GlobalState,
    context: &BlockContext,
    mut deposit_cells: Vec<DepositRequestCell>,
    input_finalized_assets: BTreeMap<H256, u128>,
) -> Result<(), Error> {
    // collect output custodian cells
    let (finalized_custodian_cells, unfinalized_custodian_cells): (Vec<_>, Vec<_>) =
        collect_custodian_locks(&context.rollup_type_hash, config, Source::Output)?
            .into_iter()
            .partition(|cell| {
                is_finalized(
                    config,
                    prev_global_state,
                    &Timepoint::from_full_value(cell.args.deposit_block_timepoint().unpack()),
                )
            });
    // check deposits request cells == unfinalized custodian cells
    // check l2block's timepoint == unfinalized custodian cells' deposit_block_timepoint
    // check l2block's block_hash == unfinalized custodian cells' deposit_block_hash
    for custodian_cell in unfinalized_custodian_cells {
        let index = deposit_cells
            .iter()
            .position(|cell| {
                custodian_cell.args.deposit_lock_args() == cell.args
                    && custodian_cell.args.deposit_block_hash() == context.block_hash.pack()
                    && custodian_cell.args.deposit_block_timepoint().unpack()
                        == context.block_timepoint().full_value()
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

fn check_layer2_deposit(
    rollup_type_hash: &H256,
    config: &RollupConfig,
    kv_state: &mut KVState,
    deposit_cells: &[DepositRequestCell],
) -> Result<(), Error> {
    let registry_ctx = gw_common::registry::context::RegistryContext::new(
        config.allowed_eoa_type_hashes().into_iter().collect(),
    );
    for request in deposit_cells {
        // check that account's script is a valid EOA script
        if request.account_script.hash_type() != ScriptHashType::Type.into() {
            return Err(Error::UnknownEOAScript);
        }
        let registry_id: u32 = request.args.registry_id().unpack();

        // find or create EOA
        let address = match kv_state.get_account_id_by_script_hash(&request.account_script_hash)? {
            Some(_id) => {
                // account is exist, query registry address
                kv_state
                    .get_registry_address_by_script_hash(registry_id, &request.account_script_hash)?
                    .ok_or(Error::RegistryAddressNotFound)?
            }
            None => {
                // account isn't exist
                let _new_id = kv_state.create_account(request.account_script_hash)?;
                let script = &request.account_script;
                let addr = registry_ctx.extract_registry_address_from_deposit(
                    registry_id,
                    &script.code_hash(),
                    &script.args().raw_data(),
                )?;
                // mapping addr to script hash
                kv_state.mapping_registry_address_to_script_hash(
                    addr.clone(),
                    request.account_script_hash,
                )?;
                addr
            }
        };

        // mint CKB
        kv_state.mint_sudt(
            CKB_SUDT_ACCOUNT_ID,
            &address,
            CKBCapacity::from_layer1(request.value.capacity).to_layer2(),
        )?;
        if request.value.sudt_script_hash.as_slice() == CKB_SUDT_SCRIPT_ARGS {
            if request.value.amount != 0 {
                // SUDT amount must equals to zero if sudt script hash is equals to CKB_SUDT_SCRIPT_ARGS
                return Err(Error::InvalidDepositCell);
            }
            continue;
        }
        // find or create Simple UDT account
        let l2_sudt_script =
            build_l2_sudt_script(rollup_type_hash, config, &request.value.sudt_script_hash)
                .ok_or(Error::InvalidDepositCell)?;
        let l2_sudt_script_hash: [u8; 32] = l2_sudt_script.hash();
        let sudt_id = match kv_state.get_account_id_by_script_hash(&l2_sudt_script_hash.into())? {
            Some(id) => id,
            None => kv_state.create_account(l2_sudt_script_hash.into())?,
        };
        // prevent fake CKB SUDT, the caller should filter these invalid deposits
        if sudt_id == CKB_SUDT_ACCOUNT_ID {
            return Err(Error::InvalidDepositCell);
        }
        // mint SUDT
        kv_state.mint_sudt(sudt_id, &address, request.value.amount.into())?;
    }

    Ok(())
}

fn check_layer2_withdrawal(
    rollup_type_hash: &H256,
    config: &RollupConfig,
    kv_state: &mut KVState,
    block: &L2BlockReader,
) -> Result<(), Error> {
    /// Pay fee to block producer
    fn pay_fee(
        kv_state: &mut KVState,
        payer_address: &RegistryAddress,
        block_producer_address: &RegistryAddress,
        amount: U256,
    ) -> Result<(), Error> {
        kv_state.burn_sudt(CKB_SUDT_ACCOUNT_ID, payer_address, amount)?;
        kv_state.mint_sudt(CKB_SUDT_ACCOUNT_ID, block_producer_address, amount)?;
        Ok(())
    }

    let withdrawals = block.withdrawals();
    // return ok if no withdrawals
    if withdrawals.is_empty() {
        return Ok(());
    }

    let block_producer_address = {
        let block_producer: Bytes = block.raw().block_producer().unpack();
        RegistryAddress::from_slice(&block_producer).ok_or(Error::Encoding)?
    };

    for request in withdrawals.iter() {
        let raw = request.raw();
        // check chain_id

        if raw.chain_id().unpack() != config.chain_id().unpack() {
            debug!("withdrawal with wrong chain_id");
            return Err(Error::InvalidWithdrawalRequest);
        }

        // find EOA
        let account_script_hash: H256 = raw.account_script_hash().unpack();
        let id = kv_state
            .get_account_id_by_script_hash(&account_script_hash)?
            .ok_or(StateError::MissingKey)?;
        let address = kv_state
            .get_registry_address_by_script_hash(raw.registry_id().unpack(), &account_script_hash)?
            .ok_or(Error::RegistryAddressNotFound)?;
        // pay fee
        {
            let fee = raw.fee().unpack();
            pay_fee(kv_state, &address, &block_producer_address, fee.into())?;
        }
        // burn CKB
        kv_state.burn_sudt(
            CKB_SUDT_ACCOUNT_ID,
            &address,
            CKBCapacity::from_layer1(raw.capacity().unpack()).to_layer2(),
        )?;
        let nonce = kv_state.get_nonce(id)?;
        // check nonce
        let withdrawal_nonce: u32 = raw.nonce().unpack();
        if nonce != withdrawal_nonce {
            return Err(Error::InvalidWithdrawalRequest);
        }
        // withdraw Simple UDT account
        match build_l2_sudt_script(rollup_type_hash, config, &raw.sudt_script_hash().unpack()) {
            Some(script) => {
                let l2_sudt_script_hash = script.hash();
                let sudt_id = kv_state
                    .get_account_id_by_script_hash(&l2_sudt_script_hash.into())?
                    .ok_or(StateError::MissingKey)?;
                // burn sudt
                kv_state.burn_sudt(sudt_id, &address, raw.amount().unpack().into())?;
            }
            None if raw.amount().unpack() != 0 => {
                // Invalid Simple UDT withdraw
                return Err(Error::InvalidWithdrawalRequest);
            }
            None => {
                // Only withdraw CKB
            }
        }
        // update nonce
        kv_state.set_nonce(id, nonce.saturating_add(1))?;
    }

    Ok(())
}

fn load_block_context_and_state<'a>(
    rollup_type_hash: H256,
    rollup_config: &RollupConfig,
    tree_buffer: &'a mut [Pair],
    kv_state_proof: &'a Bytes,
    l2block: &L2BlockReader,
    prev_global_state: &GlobalState,
    post_global_state: &GlobalState,
) -> Result<(BlockContext, KVState<'a>), Error> {
    let raw_block = l2block.raw();

    // Check pre block merkle proof
    let number: u64 = raw_block.number().unpack();
    let expected_number: u64 = prev_global_state.block().count().unpack();
    if number != expected_number {
        debug!(
            "[check block context] block number error, number: {}, expected_number: {}",
            number, expected_number
        );
        return Err(Error::InvalidBlock);
    }

    let timestamp: u64 = raw_block.timestamp().unpack();
    check_block_timestamp(rollup_config, post_global_state, timestamp)?;

    // verify parent block hash
    if raw_block.parent_block_hash().as_slice() != prev_global_state.tip_block_hash().as_slice() {
        debug!("[check block context] parent block hash error");
        return Err(Error::InvalidBlock);
    }

    // verify prev block merkle proof
    let block_smt_key = RawL2Block::compute_smt_key(number);
    let block_proof: Bytes = l2block.block_proof().unpack();
    {
        let prev_block_root: [u8; 32] = prev_global_state.block().merkle_root().unpack();

        let mut buf = [Pair::default(); 256];
        let mut block_tree = Tree::new(&mut buf);
        block_tree
            .update(&block_smt_key, &H256::zero().into())
            .map_err(|err| {
                debug!("[verify block exist] update kv error: {}", err);
                Error::MerkleProof
            })?;
        block_tree
            .verify(&prev_block_root, &block_proof)
            .map_err(|err| {
                debug!("[verify block exist] merkle verify error: {}", err);
                Error::MerkleProof
            })?;
    }

    // Check post block merkle proof
    if number + 1 != post_global_state.block().count().unpack() {
        debug!("[check block context] post global state block count error");
        return Err(Error::InvalidBlock);
    }

    let post_block_root: [u8; 32] = post_global_state.block().merkle_root().unpack();
    let block_hash: H256 = raw_block.hash().into();
    // verify prev block merkle proof
    {
        let mut buf = [Pair::default(); 256];
        let mut block_tree = Tree::new(&mut buf);
        block_tree
            .update(&block_smt_key, &block_hash.into())
            .map_err(|err| {
                debug!("[check block context] update kv error: {}", err);
                Error::MerkleProof
            })?;
        block_tree
            .verify(&post_block_root, &block_proof)
            .map_err(|err| {
                debug!("[check block context] merkle verify error: {}", err);
                Error::MerkleProof
            })?;
    }

    // Check prev account state
    if raw_block.prev_account().as_slice() != prev_global_state.account().as_slice() {
        debug!("[check block context] block's prev account error");
        return Err(Error::InvalidBlock);
    }

    // Check post account state
    // Note: Because of the optimistic mechanism, we do not need to verify post account merkle root
    if raw_block.post_account().as_slice() != post_global_state.account().as_slice() {
        return Err(Error::InvalidPostGlobalState);
    }

    // Generate context
    let post_version: u8 = post_global_state.version().into();
    let account_count: u32 = prev_global_state.account().count().unpack();
    let prev_account_root = prev_global_state.account().merkle_root().unpack();

    // Check pre account merkle proof
    let kv_state = KVState::build(
        tree_buffer,
        l2block.kv_state(),
        kv_state_proof,
        account_count,
        Some(prev_account_root),
    )?;
    if !kv_state.is_empty() && kv_state.calculate_root()? != prev_account_root {
        debug!("Block context wrong, kv state doesn't match the prev_account_root");
        return Err(Error::MerkleProof);
    }

    let context = BlockContext {
        number,
        timestamp,
        rollup_type_hash,
        block_hash,
        prev_account_root,
        post_version,
        finality_time_in_ms: finality_time_in_ms(&rollup_config),
    };

    debug!(
        "[load_block_context_and_state] BlockContext: {{
            rollup_type_hash: {:?},
            number: {},
            timestamp: {},
            block_hash: {:#x},
            post_version: {},
            prev_account_root: {:#x},
            finality_time_in_ms: {}
        }}",
        context.rollup_type_hash.pack(),
        context.number,
        context.timestamp,
        context.block_hash.pack(),
        context.post_version,
        context.prev_account_root.pack(),
        context.finality_time_in_ms
    );

    Ok((context, kv_state))
}

fn verify_block_producer(
    config: &RollupConfig,
    context: &BlockContext,
    block: &L2BlockReader,
) -> Result<(), Error> {
    let raw_block = block.raw();
    let owner_lock_hash = raw_block.stake_cell_owner_lock_hash();
    // make sure we have one stake cell in the output
    let output_stake_cell = find_block_producer_stake_cell(
        &context.rollup_type_hash,
        config,
        Source::Output,
        &owner_lock_hash,
    )?
    .ok_or(Error::InvalidStakeCell)?;
    // check stake cell capacity
    let required_staking_capacity: u64 = config.required_staking_capacity().unpack();
    if output_stake_cell.capacity < required_staking_capacity {
        debug!(
            "[verify block producer] stake cell's capacity is insufficient {} {}",
            output_stake_cell.capacity, required_staking_capacity
        );
        return Err(Error::InvalidStakeCell);
    }
    // make sure input stake cell is identical to the output stake cell if we have one
    if let Some(input_stake_cell) = find_block_producer_stake_cell(
        &context.rollup_type_hash,
        config,
        Source::Input,
        &owner_lock_hash,
    )? {
        let expected_stake_lock_args = input_stake_cell
            .args
            .as_builder()
            .stake_block_timepoint(context.block_timepoint().full_value().pack())
            .build();
        if expected_stake_lock_args != output_stake_cell.args
            || input_stake_cell.capacity > output_stake_cell.capacity
        {
            debug!("the output stake cell isn't corresponded to the input one");
            return Err(Error::InvalidStakeCell);
        }
    }

    Ok(())
}

fn check_state_checkpoints(block: &L2BlockReader) -> Result<(), Error> {
    let raw_block = block.raw();
    let checkpoint_list = raw_block.state_checkpoint_list();

    let transactions = block.transactions();
    let withdrawals = block.withdrawals();

    if checkpoint_list.len() != withdrawals.len() + transactions.len() {
        debug!(
            "Wrong checkpoint length, checkpoints_list: {}, withdrawals: {} transactions: {}",
            checkpoint_list.len(),
            withdrawals.len(),
            transactions.len()
        );
        return Err(Error::InvalidStateCheckpoint);
    }

    // check post state
    let last_state_checkpoint = if transactions.is_empty() {
        raw_block.submit_transactions().prev_state_checkpoint()
    } else {
        // return last transaction state checkpoint
        checkpoint_list
            .iter()
            .last()
            .ok_or(Error::InvalidStateCheckpoint)?
    };
    let block_state_checkpoint: Byte32 = {
        let post_account_state = raw_block.post_account();
        calculate_state_checkpoint(
            &post_account_state.merkle_root().unpack(),
            post_account_state.count().unpack(),
        )
        .pack()
    };
    if last_state_checkpoint.as_slice() != block_state_checkpoint.as_slice() {
        debug!(
            "Mismatch last_state_checkpoint: {:?}, block_state_checkpoint: {:?}",
            last_state_checkpoint, block_state_checkpoint
        );
        return Err(Error::InvalidStateCheckpoint);
    }

    Ok(())
}

fn check_block_transactions(
    block: &L2BlockReader,
    kv_state: &KVState,
    post_version: u8,
) -> Result<(), Error> {
    // check tx_witness_root
    let raw_block = block.raw();

    let submit_transactions = raw_block.submit_transactions();
    let tx_witness_root: H256 = submit_transactions.tx_witness_root().unpack();
    let tx_count: u32 = submit_transactions.tx_count().unpack();

    if tx_count != block.transactions().len() as u32 {
        debug!(
            "Mismatch tx_count, tx_count: {} block.transactions.len: {}",
            tx_count,
            block.transactions().len()
        );
        return Err(Error::InvalidBlock);
    }

    let leaves = block
        .transactions()
        .iter()
        .enumerate()
        .map(|(idx, tx)| ckb_merkle_leaf_hash(idx as u32, &tx.witness_hash().into()))
        .collect();
    let merkle_root: H256 = calculate_ckb_merkle_root(leaves)?;
    if tx_witness_root != merkle_root {
        debug!("failed to check block tx_witness_root");
        return Err(Error::MerkleProof);
    }

    // check current account tree state
    let prev_state_checkpoint: H256 = submit_transactions.prev_state_checkpoint().unpack();
    if kv_state.calculate_state_checkpoint()? != prev_state_checkpoint {
        debug!("submit_transactions.prev_state_checkpoint isn't equals to the state checkpoint calculated from context");
        return Err(Error::InvalidStateCheckpoint);
    }

    let block_post_state_root = {
        let account = raw_block.post_account();
        calculate_state_checkpoint(&account.merkle_root().unpack(), account.count().unpack())
    };
    let is_transactions_empty = block.transactions().is_empty();

    // When `block.transactions` is empty, the `block.post_account` and
    // `post_global_state.account` should be equivalent to `prev_state_checkpoint`
    if is_transactions_empty && prev_state_checkpoint != block_post_state_root {
        debug!(
            "Invalid block_post_state_root, block.transactions is empty, prev_state_checkpoint != block_post_state_root, {:x} != {:x}",
            prev_state_checkpoint.pack(), block_post_state_root.pack()
        );
        return Err(Error::InvalidStateCheckpoint);
    }

    // When `block.transactions` is not empty, the `block.post_account` and
    // `post_global_state.account` should be equivalent to the last `block.state_checkpoint_list`
    if !is_transactions_empty && Fork::enforce_correctness_of_state_checkpoint_list(post_version) {
        let last_state_checkpoint = &raw_block
            .state_checkpoint_list()
            .iter()
            .last()
            .as_ref()
            .map(Unpack::<H256>::unpack)
            .ok_or(Error::InvalidStateCheckpoint)?;
        if last_state_checkpoint != &block_post_state_root {
            debug!(
                "Invalid block_post_state_root, last_state_checkpoint != block_post_state_root, {:x} != {:x}",
                last_state_checkpoint.pack(), block_post_state_root.pack()
            );
            return Err(Error::InvalidStateCheckpoint);
        }
    }

    Ok(())
}

fn check_block_withdrawals(block: &L2BlockReader) -> Result<(), Error> {
    // check withdrawal_witness_root
    let submit_withdrawals = block.raw().submit_withdrawals();

    let withdrawal_witness_root: H256 = submit_withdrawals.withdrawal_witness_root().unpack();
    let withdrawal_count: u32 = submit_withdrawals.withdrawal_count().unpack();

    if withdrawal_count != block.withdrawals().len() as u32 {
        debug!(
            "Mismatch withdrawal_count, withdrawal_count: {} block.withdrawals.len: {}",
            withdrawal_count,
            block.withdrawals().len()
        );
        return Err(Error::InvalidBlock);
    }

    let leaves = block
        .withdrawals()
        .iter()
        .enumerate()
        .map(|(idx, withdrawal)| {
            ckb_merkle_leaf_hash(idx as u32, &withdrawal.witness_hash().into())
        })
        .collect();
    let merkle_root = calculate_ckb_merkle_root(leaves)?;
    if withdrawal_witness_root != merkle_root {
        debug!("failed to check block withdrawal_witness_root");
        return Err(Error::MerkleProof);
    }

    Ok(())
}

// Assert l2block.timestamp == post_global_state.tip_block_timestamp
// Assert l2block.timestamp <= l1tx.since
// Assert l2block.timestamp <=
//      post_global_state.last_finalized_timepoint
//      + rollup_config.finality_time_in_ms()
//      + 4h
// Assert l2block.timestamp >=
//      post_global_state.last_finalized_timepoint
//      + rollup_config.finality_time_in_ms()
//      - 4h
fn check_block_timestamp(
    rollup_config: &RollupConfig,
    post_global_state: &GlobalState,
    block_timestamp: u64,
) -> Result<(), Error> {
    let post_block_timestamp: u64 = post_global_state.tip_block_timestamp().unpack();
    if block_timestamp != post_block_timestamp {
        debug!(
            "[check_block_timestamp] block.timestamp is not same as post_global_state.tip_block_timestamp, {} != {}",
            block_timestamp, post_block_timestamp
        );
        return Err(Error::InvalidBlock);
    }

    let post_version: u8 = post_global_state.version().into();
    if Fork::enforce_block_timestamp_lower_than_since(post_version) {
        let rollup_input_since = Since::new(load_input_since(0, Source::GroupInput)?);
        let rollup_input_timestamp = match (
            rollup_input_since.is_absolute(),
            rollup_input_since.extract_lock_value(),
        ) {
            (true, Some(LockValue::Timestamp(time_ms))) => time_ms,
            _ => return Err(Error::InvalidSince),
        };
        if rollup_input_timestamp <= block_timestamp {
            return Err(Error::InvalidPostGlobalState);
        }
    }

    if Fork::enforce_block_timestamp_in_l1_backbone_range(post_version) {
        // 4 hours, 4 * 60 * 60 * 1000 = 14400000ms
        const BACKBONE_BIAS: u64 = 14400000;
        let backbone = {
            match Timepoint::from_full_value(
                post_global_state.last_finalized_block_number().unpack(),
            ) {
                Timepoint::BlockNumber(_) => unreachable!(),
                Timepoint::Timestamp(ts) => ts + finality_time_in_ms(rollup_config),
            }
        };
        if !(backbone.saturating_sub(BACKBONE_BIAS) <= block_timestamp
            && block_timestamp <= backbone.saturating_add(BACKBONE_BIAS))
        {
            debug!(
                "[check_block_timestamp] block.timestamp is out of available range, block.timestamp: {}, backbone range: [{}-{}, {}+{}]",
                block_timestamp,
                backbone, BACKBONE_BIAS,
                backbone, BACKBONE_BIAS
            );
            return Err(Error::InvalidBlock);
        }
    }

    Ok(())
}

fn check_global_state_tip_block_timestamp(
    prev_global_state: &GlobalState,
    post_global_state: &GlobalState,
) -> Result<(), Error> {
    let prev_block_timestamp: u64 = prev_global_state.tip_block_timestamp().unpack();
    let post_block_timestamp: u64 = post_global_state.tip_block_timestamp().unpack();
    if !(prev_block_timestamp < post_block_timestamp) {
        debug!(
            "[check_global_state_tip_block_timestamp] prev_block_timestamp: {}, post_block_timestamp: {}",
            prev_block_timestamp,
            post_block_timestamp
        );
        return Err(Error::InvalidPostGlobalState);
    }
    Ok(())
}

fn check_global_state_last_finalized_timepoint(
    rollup_config: &RollupConfig,
    context: &BlockContext,
    prev_global_state: &GlobalState,
    post_global_state: &GlobalState,
) -> Result<(), Error> {
    // skip
    // if context.post_version == 0 { }

    if prev_global_state.last_finalized_block_number().unpack()
        > post_global_state.last_finalized_block_number().unpack()
    {
        debug!(
            "check_global_state_last_finalized_timepoint prev last_finalized_block_number > post last_finalized_block_number, {} > {}",
            prev_global_state.last_finalized_block_number().unpack(),
            post_global_state.last_finalized_block_number().unpack()
        );
        return Err(Error::InvalidPostGlobalState);
    }

    let last_finalized_timepoint =
        Timepoint::from_full_value(post_global_state.last_finalized_block_number().unpack());

    match (
        Fork::use_timestamp_as_timepoint(context.post_version),
        last_finalized_timepoint,
    ) {
        (false, Timepoint::BlockNumber(last_finalized_block_number)) => {
            let finality_as_blocks = rollup_config.finality_blocks().unpack();
            if last_finalized_block_number != context.number.saturating_sub(finality_as_blocks) {
                return Err(Error::InvalidPostGlobalState);
            }
        }
        (true, Timepoint::Timestamp(last_finalized_timestamp)) => {
            let rollup_input_since = Since::new(load_input_since(0, Source::GroupInput)?);
            let rollup_input_timestamp = match (
                rollup_input_since.is_absolute(),
                rollup_input_since.extract_lock_value(),
            ) {
                (true, Some(LockValue::Timestamp(time_ms))) => time_ms,
                _ => return Err(Error::InvalidSince),
            };
            if !(last_finalized_timestamp + finality_time_in_ms(rollup_config)
                < rollup_input_timestamp)
            {
                return Err(Error::InvalidPostGlobalState);
            }
        }
        (enabled, timepoint) => {
            debug!(
                "timestamp_as_timepoint feature switch is not matched, enabled: {}, timepoint: {}",
                enabled,
                timepoint.full_value()
            );
            return Err(Error::InvalidPostGlobalState);
        }
    }
    Ok(())
}

/// Verify Deposit & Withdrawal
pub fn verify(
    rollup_type_hash: H256,
    config: &RollupConfig,
    block: &L2BlockReader,
    prev_global_state: &GlobalState,
    post_global_state: &GlobalState,
) -> Result<(), Error> {
    check_status(prev_global_state, Status::Running)?;

    // check checkpoints
    check_state_checkpoints(block)?;

    // Check withdrawals root
    check_block_withdrawals(block)?;

    let mut tree_buffer = [Pair::default(); GW_MAX_KV_PAIRS];
    let kv_state_proof: Bytes = block.kv_state_proof().unpack();

    let (context, mut kv_state) = load_block_context_and_state(
        rollup_type_hash,
        config,
        &mut tree_buffer,
        &kv_state_proof,
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
    let deposit_cells = collect_deposit_locks(&context.rollup_type_hash, config, Source::Input)?;
    // Check new cells and reverted cells: deposit / withdrawal / custodian
    let withdrawal_requests_vec = block.withdrawals();
    let withdrawal_requests = withdrawal_requests_vec.iter().collect();
    check_withdrawal_cells(&context, withdrawal_requests, &withdrawal_cells)?;
    let input_finalized_assets =
        check_input_custodian_cells(config, prev_global_state, &context, withdrawal_cells)?;
    check_output_custodian_cells(
        config,
        prev_global_state,
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
    check_layer2_withdrawal(&rollup_type_hash, config, &mut kv_state, block)?;
    // Mint token: deposit requests -> layer2 SUDT
    check_layer2_deposit(&rollup_type_hash, config, &mut kv_state, &deposit_cells)?;
    // Check transactions
    check_block_transactions(block, &kv_state, context.post_version)?;

    check_global_state_tip_block_timestamp(prev_global_state, post_global_state)?;
    check_global_state_last_finalized_timepoint(
        &config,
        &context,
        prev_global_state,
        post_global_state,
    )?;

    // Verify Post state
    let actual_post_global_state = {
        // because of the optimistic challenge mechanism,
        // we just believe the post account in the block,
        // if the post account state is invalid then someone will send a challenge
        let account_merkle_state = block.raw().post_account();
        // we have verified the post block merkle state
        let block_merkle_state = post_global_state.block();

        // check_global_state_tip_block_timestamp() has checked post_global_state.tip_block_timestamp
        // check_global_state_last_finalized_timepoint() has checked post_global_state.last_finalized_block_number
        let last_finalized_block_number = post_global_state.last_finalized_block_number();
        let tip_block_timestamp = if context.post_version == 0 {
            0
        } else {
            context.timestamp
        };

        prev_global_state
            .clone()
            .as_builder()
            .account(account_merkle_state.to_entity())
            .block(block_merkle_state)
            .tip_block_hash(context.block_hash.pack())
            .tip_block_timestamp(tip_block_timestamp.pack())
            .last_finalized_block_number(last_finalized_block_number)
            .version(context.post_version.into())
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
    if reverted_block_hashes.is_empty() && reverted_block_proof.is_empty() {
        return Ok(());
    }
    let mut buf = [Pair::default(); 256];
    let mut block_tree = Tree::new(&mut buf);
    for key in reverted_block_hashes {
        block_tree
            .update(&key.into(), &H256::one().into())
            .map_err(|err| {
                debug!("[verify reverted block] update kv error: {}", err);
                Error::MerkleProof
            })?;
    }
    block_tree
        .verify(&reverted_block_root, &reverted_block_proof)
        .map_err(|err| {
            debug!("[verify reverted block] merkle verify error: {}", err);
            Error::MerkleProof
        })?;
    Ok(())
}
