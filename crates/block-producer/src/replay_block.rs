use anyhow::{anyhow, bail, Result};
use ckb_types::prelude::{Builder, Entity};
use gw_common::merkle_utils::calculate_state_checkpoint;
use gw_common::smt::SMT;
use gw_common::state::State;
use gw_common::H256;
use gw_generator::constants::L2TX_MAX_CYCLES;
use gw_generator::traits::StateExt;
use gw_generator::Generator;
use gw_store::chain_view::ChainView;
use gw_store::smt::mem_smt_store::MemSMTStore;
use gw_store::state::mem_state_db::{MemStateContext, MemStateTree};
use gw_store::transaction::StoreTransaction;
use gw_types::packed::{BlockInfo, DepositRequest, L2Block, L2Transaction, RawL2Block};
use gw_types::prelude::Unpack;

use std::collections::HashMap;

pub struct InvalidState {
    tx: L2Transaction,
    kv: HashMap<H256, H256>,
}

pub enum ReplayError {
    State(InvalidState),
    Db(gw_db::error::Error),
}

pub struct ReplayBlock;

impl ReplayBlock {
    pub fn replay(
        db: &StoreTransaction,
        generator: &Generator,
        block: &L2Block,
        deposits: &[DepositRequest],
    ) -> Result<()> {
        let raw_block = block.raw();
        let block_info = get_block_info(&raw_block);
        let block_number = raw_block.number().unpack();
        log::info!("replay block {}", block_number);

        let parent_block_hash: H256 = raw_block.parent_block_hash().unpack();
        let parent_block = db
            .get_block(&parent_block_hash)?
            .ok_or_else(|| anyhow!("replay parent block not found"))?;

        let mut state = {
            let parent_post_state = parent_block.raw().post_account();
            let smt = db.account_smt_store()?;
            let mem_smt_store = MemSMTStore::new(smt);
            let tree = SMT::new(parent_post_state.merkle_root().unpack(), mem_smt_store);
            let context = MemStateContext::Tip;
            MemStateTree::new(db, tree, parent_post_state.count().unpack(), context)
        };

        // apply withdrawal to state
        let withdrawal_requests: Vec<_> = block.withdrawals().into_iter().collect();
        let block_producer_id: u32 = block_info.block_producer_id().unpack();
        let state_checkpoint_list: Vec<H256> = raw_block.state_checkpoint_list().unpack();

        for (wth_idx, request) in withdrawal_requests.iter().enumerate() {
            generator.check_withdrawal_request_signature(&state, request)?;

            state.apply_withdrawal_request(
                generator.rollup_context(),
                block_producer_id,
                request,
            )?;

            let account_state = state.get_merkle_state();
            let expected_checkpoint = calculate_state_checkpoint(
                &account_state.merkle_root().unpack(),
                account_state.count().unpack(),
            );

            let block_checkpoint: H256 = match state_checkpoint_list.get(wth_idx) {
                Some(checkpoint) => *checkpoint,
                None => bail!("withdrawal {} checkpoint not found", wth_idx),
            };
            if block_checkpoint != expected_checkpoint {
                bail!("withdrawal {} checkpoint not match", wth_idx);
            }
        }

        // apply deposition to state
        state.apply_deposit_requests(generator.rollup_context(), deposits)?;
        let prev_txs_state = state.get_merkle_state();
        let expected_prev_txs_state_checkpoint = calculate_state_checkpoint(
            &prev_txs_state.merkle_root().unpack(),
            prev_txs_state.count().unpack(),
        );
        let block_prev_txs_state_checkpoint: H256 = raw_block
            .submit_transactions()
            .prev_state_checkpoint()
            .unpack();
        if block_prev_txs_state_checkpoint != expected_prev_txs_state_checkpoint {
            bail!("prev txs state checkpoint not match");
        }

        // handle transactions
        let chain_view = ChainView::new(db, parent_block_hash);
        for (tx_index, tx) in block.transactions().into_iter().enumerate() {
            generator.check_transaction_signature(&state, &tx)?;

            // check nonce
            let raw_tx = tx.raw();
            let expected_nonce = state.get_nonce(raw_tx.from_id().unpack())?;
            let actual_nonce: u32 = raw_tx.nonce().unpack();
            if actual_nonce != expected_nonce {
                bail!(
                    "tx {} nonce not match, expected {} actual {}",
                    tx_index,
                    expected_nonce,
                    actual_nonce
                );
            }

            // build call context
            // NOTICE users only allowed to send HandleMessage CallType txs
            let run_result = generator.execute_transaction(
                &chain_view,
                &state,
                &block_info,
                &raw_tx,
                L2TX_MAX_CYCLES,
            )?;

            state.apply_run_result(&run_result)?;
            let account_state = state.get_merkle_state();

            let expected_checkpoint = calculate_state_checkpoint(
                &account_state.merkle_root().unpack(),
                account_state.count().unpack(),
            );
            let checkpoint_index = withdrawal_requests.len() + tx_index;
            let block_checkpoint: H256 = match state_checkpoint_list.get(checkpoint_index) {
                Some(checkpoint) => *checkpoint,
                None => bail!("tx {} checkpoint not found", tx_index),
            };

            if block_checkpoint != expected_checkpoint {
                bail!("tx {} checkpoint not match", tx_index);
            }
        }

        Ok(())
    }
}

fn get_block_info(l2block: &RawL2Block) -> BlockInfo {
    BlockInfo::new_builder()
        .block_producer_id(l2block.block_producer_id())
        .number(l2block.number())
        .timestamp(l2block.timestamp())
        .build()
}

impl From<gw_db::error::Error> for ReplayError {
    fn from(db_err: gw_db::error::Error) -> Self {
        ReplayError::Db(db_err)
    }
}
