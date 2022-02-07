use anyhow::{anyhow, bail, Result};
use ckb_types::prelude::{Builder, Entity};
use gw_common::merkle_utils::calculate_state_checkpoint;
use gw_common::state::State;
use gw_common::H256;
use gw_generator::constants::L2TX_MAX_CYCLES;
use gw_generator::traits::StateExt;
use gw_generator::Generator;
use gw_store::chain_view::ChainView;
use gw_store::mem_pool_state::MemStore;
use gw_store::traits::chain_store::ChainStore;
use gw_store::Store;
use gw_types::packed::{BlockInfo, DepositRequest, L2Block, RawL2Block, WithdrawalRequestExtra};
use gw_types::prelude::Unpack;

pub struct ReplayBlock;

impl ReplayBlock {
    pub fn replay(
        store: &Store,
        generator: &Generator,
        block: &L2Block,
        deposits: &[DepositRequest],
        withdrawals: &[WithdrawalRequestExtra],
    ) -> Result<()> {
        let raw_block = block.raw();
        let block_info = get_block_info(&raw_block);
        let block_number = raw_block.number().unpack();
        log::info!("replay block {}", block_number);

        let parent_block_hash: H256 = raw_block.parent_block_hash().unpack();
        let snap = store.get_snapshot();
        let parent_block = snap
            .get_block(&parent_block_hash)?
            .ok_or_else(|| anyhow!("replay parent block not found"))?;

        let mem_store = MemStore::new(snap);
        let mut state = mem_store.state()?;
        {
            let parent_post_state = parent_block.raw().post_account();
            assert_eq!(
                parent_post_state,
                state.merkle_state()?,
                "merkle state should equals to parent block"
            );
        };

        // apply withdrawal to state
        let block_producer_id: u32 = block_info.block_producer_id().unpack();
        let state_checkpoint_list: Vec<H256> = raw_block.state_checkpoint_list().unpack();

        for (wth_idx, withdrawal) in withdrawals.iter().enumerate() {
            generator.check_withdrawal_signature(&state, withdrawal)?;

            state.apply_withdrawal_request(
                generator.rollup_context(),
                block_producer_id,
                &withdrawal.request(),
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
        let db = store.begin_transaction();
        let chain_view = ChainView::new(&db, parent_block_hash);
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
                None,
            )?;

            state.apply_run_result(&run_result)?;
            let account_state = state.get_merkle_state();

            let expected_checkpoint = calculate_state_checkpoint(
                &account_state.merkle_root().unpack(),
                account_state.count().unpack(),
            );
            let checkpoint_index = withdrawals.len() + tx_index;
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
