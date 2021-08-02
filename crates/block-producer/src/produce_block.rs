//! Block producer
//! Block producer assemble serveral Godwoken components into a single executor.
//! A block producer can act without the ability of produce block.

use crate::withdrawal::AvailableCustodians;

use anyhow::{anyhow, Result};
use gw_common::{
    h256_ext::H256Ext,
    merkle_utils::{calculate_merkle_root, calculate_state_checkpoint},
    smt::Blake2bHasher,
    state::State,
    H256,
};
use gw_generator::{traits::StateExt, Generator};
use gw_store::{
    chain_view::ChainView,
    state_db::{CheckPoint, StateDBMode, StateDBTransaction, SubState},
    transaction::StoreTransaction,
};
use gw_types::{
    core::Status,
    packed::{
        AccountMerkleState, BlockInfo, BlockMerkleState, DepositRequest, GlobalState, L2Block,
        L2Transaction, RawL2Block, SubmitTransactions, SubmitWithdrawals, TxReceipt,
        WithdrawalRequest,
    },
    prelude::*,
};

pub struct ProduceBlockResult {
    pub block: L2Block,
    pub global_state: GlobalState,
    pub unused_transactions: Vec<L2Transaction>,
    pub unused_withdrawal_requests: Vec<WithdrawalRequest>,
    pub l2tx_offchain_used_cycles: u64,
}

pub struct ProduceBlockParam<'a> {
    pub db: StoreTransaction,
    pub generator: &'a Generator,
    pub block_producer_id: u32,
    pub stake_cell_owner_lock_hash: H256,
    pub timestamp: u64,
    pub txs: Vec<L2Transaction>,
    pub deposit_requests: Vec<DepositRequest>,
    pub withdrawal_requests: Vec<WithdrawalRequest>,
    pub parent_block: &'a L2Block,
    pub reverted_block_root: H256,
    pub rollup_config_hash: &'a H256,
    pub max_withdrawal_capacity: u128,
    pub available_custodians: AvailableCustodians,
}

/// Produce block
/// this method take txs & withdrawal requests from tx pool and produce a new block
/// the package method should packs the items in order:
/// withdrawals, then deposits, finally the txs. Thus, the state-validator can verify this correctly
pub fn produce_block(param: ProduceBlockParam<'_>) -> Result<ProduceBlockResult> {
    let ProduceBlockParam {
        db,
        generator,
        block_producer_id,
        timestamp,
        txs,
        deposit_requests,
        withdrawal_requests,
        parent_block,
        reverted_block_root,
        rollup_config_hash,
        max_withdrawal_capacity,
        stake_cell_owner_lock_hash,
        available_custodians,
    } = param;
    let rollup_context = generator.rollup_context();
    let parent_block_number: u64 = parent_block.raw().number().unpack();
    let parent_block_hash = parent_block.hash().into();
    // create overlay storage
    let state_db = {
        let tip_block_hash = db.get_tip_block_hash()?;
        assert_eq!(parent_block_hash, tip_block_hash);
        StateDBTransaction::from_checkpoint(
            &db,
            CheckPoint::from_block_hash(&db, tip_block_hash, SubState::Block)?,
            StateDBMode::ReadOnly,
        )?
    };
    let mut state = state_db.account_state_tree()?;
    // track state changes
    state.tracker_mut().enable();
    let prev_account_state_root = state.calculate_root()?;
    let prev_account_state_count = state.get_account_count()?;
    // verify the withdrawals
    let mut used_withdrawal_requests = Vec::with_capacity(withdrawal_requests.len());
    let mut unused_withdrawal_requests = Vec::with_capacity(withdrawal_requests.len());
    let mut total_withdrawal_capacity: u128 = 0;
    let mut state_checkpoint_list: Vec<H256> = Vec::new();
    let mut withdrawal_verifier =
        crate::withdrawal::Generator::new(rollup_context, available_custodians);
    for request in withdrawal_requests {
        // check withdrawal request
        if let Err(err) = generator.check_withdrawal_request_signature(&state, &request) {
            log::info!("[produce_block] withdrawal signature error: {:?}", err);
            unused_withdrawal_requests.push(request);
            continue;
        }
        if let Err(err) = generator.verify_withdrawal_request(&state, &request) {
            log::info!("[produce_block] withdrawal verification error: {:?}", err);
            unused_withdrawal_requests.push(request);
            continue;
        }
        let capacity: u64 = request.raw().capacity().unpack();
        let new_total_withdrwal_capacity = total_withdrawal_capacity
            .checked_add(capacity as u128)
            .ok_or_else(|| anyhow!("total withdrawal capacity overflow"))?;
        // skip package withdrwal if overdraft the Rollup capacity
        if new_total_withdrwal_capacity > max_withdrawal_capacity {
            log::info!(
                "[produce_block] max_withdrawal_capacity({}) is not enough to withdraw({})",
                max_withdrawal_capacity,
                new_total_withdrwal_capacity
            );
            unused_withdrawal_requests.push(request);
            continue;
        }
        total_withdrawal_capacity = new_total_withdrwal_capacity;

        if let Err(err) = withdrawal_verifier.include_and_verify(&request, &L2Block::default()) {
            log::info!(
                "[produce_block] withdrawal contextual verification failed : {}",
                err
            );
            unused_withdrawal_requests.push(request);
            continue;
        }

        // update the state
        match state.apply_withdrawal_request(rollup_context, block_producer_id, &request) {
            Ok(_) => {
                used_withdrawal_requests.push(request);
                state_checkpoint_list.push(state.calculate_state_checkpoint()?);
            }
            Err(err) => {
                log::info!("[produce_block] withdrawal execution failed : {}", err);
                unused_withdrawal_requests.push(request);
            }
        }
    }
    // update deposits
    state.apply_deposit_requests(rollup_context, &deposit_requests)?;
    // calculate state after withdrawals & deposits
    let prev_state_check_point = state.calculate_state_checkpoint()?;
    state.tracker_mut().disable();
    // execute txs
    let mut tx_receipts = Vec::with_capacity(txs.len());
    let mut used_transactions = Vec::with_capacity(txs.len());
    let mut unused_transactions = Vec::with_capacity(txs.len());
    // build block info
    let number = parent_block_number + 1;
    let block_info = BlockInfo::new_builder()
        .number(number.pack())
        .timestamp(timestamp.pack())
        .block_producer_id(block_producer_id.pack())
        .build();
    let chain_view = ChainView::new(&db, parent_block_hash);

    let mut l2tx_offchain_used_cycles: u64 = 0;
    let has_txs = !txs.is_empty();
    for tx in txs {
        // 1. verify tx
        if let Err(err) = generator.check_transaction_signature(&state, &tx) {
            log::info!("[produce_block] check tx signature error: {:?}", err);
            unused_transactions.push(tx);
            continue;
        }
        if let Err(err) = generator.verify_transaction(&state, &tx) {
            log::info!("[produce_block] verify tx error: {:?}", err);
            unused_transactions.push(tx);
            continue;
        }
        // 2. execute txs
        let raw_tx = tx.raw();
        let run_result =
            match generator.execute_transaction(&chain_view, &state, &block_info, &raw_tx) {
                Ok(run_result) => run_result,
                Err(err) => {
                    log::info!(
                        "[produce_block] execute tx {} error: {:?}",
                        hex::encode(&tx.hash()),
                        err
                    );
                    unused_transactions.push(tx);
                    continue;
                }
            };
        // 3. apply tx state
        state.apply_run_result(&run_result)?;
        // 4. build tx receipt
        let tx_witness_hash = tx.witness_hash();
        let tx_post_state = {
            let account_root = state.calculate_root()?;
            let account_count = state.get_account_count()?;
            AccountMerkleState::new_builder()
                .merkle_root(account_root.pack())
                .count(account_count.pack())
                .build()
        };
        let receipt = TxReceipt::new_builder()
            .tx_witness_hash(tx_witness_hash.pack())
            .post_state(tx_post_state)
            .read_data_hashes(
                run_result
                    .read_data
                    .iter()
                    .map(|(hash, _)| *hash)
                    .collect::<Vec<_>>()
                    .pack(),
            )
            .logs(run_result.logs.pack())
            .build();
        used_transactions.push(tx);
        tx_receipts.push(receipt);
        l2tx_offchain_used_cycles =
            l2tx_offchain_used_cycles.saturating_add(run_result.used_cycles);
    }
    assert_eq!(used_transactions.len(), tx_receipts.len());
    let touched_keys: Vec<H256> = state
        .tracker_mut()
        .touched_keys()
        .expect("track touched keys")
        .borrow()
        .clone()
        .into_iter()
        .collect();
    let post_account_state_root = state.calculate_root()?;
    let post_account_state_count = state.get_account_count()?;

    if !has_txs {
        let post_state_check_point = state.calculate_state_checkpoint()?;
        assert_eq!(prev_state_check_point, post_state_check_point);
    }

    // discard all changes
    drop(state);
    db.rollback()?;

    // assemble block
    let submit_txs = {
        let tx_witness_root = calculate_merkle_root(
            tx_receipts
                .iter()
                .map(|tx_receipt| tx_receipt.tx_witness_hash().unpack())
                .collect(),
        )
        .map_err(|err| anyhow!("merkle root error: {:?}", err))?;
        let tx_count = tx_receipts.len() as u32;
        state_checkpoint_list.extend(tx_receipts.iter().map(|tx_receipt| {
            let post_state = tx_receipt.post_state();
            calculate_state_checkpoint(
                &post_state.merkle_root().unpack(),
                post_state.count().unpack(),
            )
        }));
        SubmitTransactions::new_builder()
            .tx_witness_root(tx_witness_root.pack())
            .tx_count(tx_count.pack())
            .prev_state_checkpoint(prev_state_check_point.pack())
            .build()
    };
    let submit_withdrawals = {
        let withdrawal_witness_root = calculate_merkle_root(
            used_withdrawal_requests
                .iter()
                .map(|request| request.witness_hash().into())
                .collect(),
        )
        .map_err(|err| anyhow!("merkle root error: {:?}", err))?;
        let withdrawal_count = used_withdrawal_requests.len() as u32;
        SubmitWithdrawals::new_builder()
            .withdrawal_witness_root(withdrawal_witness_root.pack())
            .withdrawal_count(withdrawal_count.pack())
            .build()
    };
    let prev_account = AccountMerkleState::new_builder()
        .merkle_root(prev_account_state_root.pack())
        .count(prev_account_state_count.pack())
        .build();
    assert_eq!(parent_block.raw().post_account(), prev_account);
    let post_account = AccountMerkleState::new_builder()
        .merkle_root(post_account_state_root.pack())
        .count(post_account_state_count.pack())
        .build();
    assert_eq!(
        state_checkpoint_list.len(),
        used_withdrawal_requests.len() + used_transactions.len(),
        "state checkpoint len"
    );
    let raw_block = RawL2Block::new_builder()
        .number(number.pack())
        .block_producer_id(block_producer_id.pack())
        .stake_cell_owner_lock_hash(stake_cell_owner_lock_hash.pack())
        .timestamp(timestamp.pack())
        .parent_block_hash(parent_block_hash.pack())
        .post_account(post_account.clone())
        .prev_account(prev_account)
        .submit_transactions(submit_txs)
        .submit_withdrawals(submit_withdrawals)
        .state_checkpoint_list(state_checkpoint_list.pack())
        .build();
    // generate block fields from current state
    let state = state_db.account_state_tree()?;
    let kv_state: Vec<(H256, H256)> = touched_keys
        .iter()
        .map(|k| {
            state
                .get_raw(k)
                .map(|v| (*k, v))
                .map_err(|err| anyhow!("can't fetch value error: {:?}", err))
        })
        .collect::<Result<_>>()?;
    let packed_kv_state = kv_state.pack();
    let proof = if kv_state.is_empty() {
        // nothing need to prove
        Vec::new()
    } else {
        let account_smt = state_db.account_smt()?;

        account_smt
            .merkle_proof(kv_state.iter().map(|(k, _v)| *k).collect())
            .map_err(|err| anyhow!("merkle proof error: {:?}", err))?
            .compile(kv_state)?
            .0
    };
    let block_smt = db.block_smt()?;
    let block_proof = block_smt
        .merkle_proof(vec![H256::from_u64(number)])
        .map_err(|err| anyhow!("merkle proof error: {:?}", err))?
        .compile(vec![(H256::from_u64(number), H256::zero())])?;
    let block = L2Block::new_builder()
        .raw(raw_block)
        .kv_state(packed_kv_state)
        .kv_state_proof(proof.pack())
        .transactions(used_transactions.pack())
        .withdrawals(used_withdrawal_requests.pack())
        .block_proof(block_proof.0.pack())
        .build();
    let post_block = {
        let post_block_root: [u8; 32] = block_proof
            .compute_root::<Blake2bHasher>(vec![(block.smt_key().into(), block.hash().into())])?
            .into();
        let block_count = number + 1;
        BlockMerkleState::new_builder()
            .merkle_root(post_block_root.pack())
            .count(block_count.pack())
            .build()
    };
    let last_finalized_block_number =
        number.saturating_sub(rollup_context.rollup_config.finality_blocks().unpack());
    let global_state = GlobalState::new_builder()
        .account(post_account)
        .block(post_block)
        .tip_block_hash(block.hash().pack())
        .last_finalized_block_number(last_finalized_block_number.pack())
        .reverted_block_root(Into::<[u8; 32]>::into(reverted_block_root).pack())
        .rollup_config_hash(rollup_config_hash.pack())
        .status((Status::Running as u8).into())
        .build();
    Ok(ProduceBlockResult {
        block,
        global_state,
        unused_transactions,
        unused_withdrawal_requests,
        l2tx_offchain_used_cycles,
    })
}
