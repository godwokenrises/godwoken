//! Block producer
//! Block producer assemble several Godwoken components into a single executor.
//! A block producer can act without the ability of produce block.

use anyhow::{anyhow, Result};
use gw_chain::chain::Chain;
use gw_common::{
    h256_ext::H256Ext,
    merkle_utils::{calculate_ckb_merkle_root, calculate_state_checkpoint, ckb_merkle_leaf_hash},
    smt::Blake2bHasher,
    sparse_merkle_tree::CompiledMerkleProof,
    state::State,
    H256,
};
use gw_generator::Generator;
use gw_mem_pool::mem_block::MemBlock;
use gw_store::{
    state::state_db::StateContext, traits::chain_store::ChainStore, transaction::StoreTransaction,
    Store,
};
use gw_types::{
    core::Status,
    offchain::{BlockParam, DepositInfo},
    packed::{
        AccountMerkleState, BlockMerkleState, GlobalState, L2Block, RawL2Block, SubmitTransactions,
        SubmitWithdrawals, WithdrawalCursor, WithdrawalRequestExtra,
    },
    prelude::*,
};
use tracing::instrument;

#[derive(Clone)]
pub struct ProduceBlockResult {
    pub block: L2Block,
    pub global_state: GlobalState,
    pub deposit_cells: Vec<DepositInfo>,
    pub withdrawal_extras: Vec<WithdrawalRequestExtra>,
}

pub struct ProduceBlockParam {
    pub stake_cell_owner_lock_hash: H256,
    pub reverted_block_root: H256,
    pub rollup_config_hash: H256,
    pub block_param: BlockParam,
}

/// Produce block
/// this method take txs & withdrawal requests from tx pool and produce a new block
/// the package method should packs the items in order:
/// withdrawals, then deposits, finally the txs. Thus, the state-validator can verify this correctly
#[instrument(skip_all)]
pub fn produce_block(
    db: &StoreTransaction,
    generator: &Generator,
    param: ProduceBlockParam,
) -> Result<ProduceBlockResult> {
    let ProduceBlockParam {
        stake_cell_owner_lock_hash,
        reverted_block_root,
        rollup_config_hash,
        block_param:
            BlockParam {
                number,
                block_producer,
                timestamp,
                txs,
                deposits,
                withdrawals,
                parent_block,
                prev_merkle_state,
                state_checkpoint_list,
                txs_prev_state_checkpoint,
                kv_state,
                kv_state_proof,
                post_merkle_state,
                last_finalized_withdrawal,
            },
    } = param;

    let rollup_context = generator.rollup_context();
    let parent_block_hash: H256 = parent_block.hash().into();

    // assemble block
    let submit_txs = {
        let tx_witness_root = calculate_ckb_merkle_root(
            txs.iter()
                .enumerate()
                .map(|(id, tx)| ckb_merkle_leaf_hash(id as u32, &tx.witness_hash().into()))
                .collect(),
        )
        .map_err(|err| anyhow!("merkle root error: {:?}", err))?;
        let tx_count = txs.len() as u32;
        SubmitTransactions::new_builder()
            .tx_witness_root(tx_witness_root.pack())
            .tx_count(tx_count.pack())
            .prev_state_checkpoint(txs_prev_state_checkpoint.pack())
            .build()
    };
    let submit_withdrawals = {
        let withdrawal_witness_root = calculate_ckb_merkle_root(
            withdrawals
                .iter()
                .enumerate()
                .map(|(id, request)| {
                    ckb_merkle_leaf_hash(id as u32, &request.witness_hash().into())
                })
                .collect(),
        )
        .map_err(|err| anyhow!("merkle root error: {:?}", err))?;
        let withdrawal_count = withdrawals.len() as u32;
        SubmitWithdrawals::new_builder()
            .withdrawal_witness_root(withdrawal_witness_root.pack())
            .withdrawal_count(withdrawal_count.pack())
            .build()
    };
    assert_eq!(parent_block.raw().post_account(), prev_merkle_state);
    assert_eq!(
        state_checkpoint_list.len(),
        withdrawals.len() + txs.len(),
        "state checkpoint len"
    );
    let raw_block = RawL2Block::new_builder()
        .number(number.pack())
        .block_producer(block_producer.pack())
        .stake_cell_owner_lock_hash(stake_cell_owner_lock_hash.pack())
        .timestamp(timestamp.pack())
        .parent_block_hash(parent_block_hash.pack())
        .post_account(post_merkle_state.clone())
        .prev_account(prev_merkle_state)
        .submit_transactions(submit_txs)
        .submit_withdrawals(submit_withdrawals)
        .state_checkpoint_list(state_checkpoint_list.pack())
        .build();
    let block_smt = db.block_smt()?;
    let block_proof = block_smt
        .merkle_proof(vec![H256::from_u64(number)])
        .map_err(|err| anyhow!("merkle proof error: {:?}", err))?
        .compile(vec![(H256::from_u64(number), H256::zero())])?;
    let packed_kv_state = kv_state.pack();
    let withdrawal_requests = withdrawals.iter().map(|w| w.request());
    let block = L2Block::new_builder()
        .raw(raw_block)
        .kv_state(packed_kv_state)
        .kv_state_proof(kv_state_proof.pack())
        .transactions(txs.pack())
        .withdrawals(withdrawal_requests.collect::<Vec<_>>().pack())
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
        .account(post_merkle_state)
        .block(post_block)
        .tip_block_hash(block.hash().pack())
        .tip_block_timestamp(block.raw().timestamp())
        .last_finalized_block_number(last_finalized_block_number.pack())
        .finalized_withdrawal_cursor(last_finalized_withdrawal)
        .reverted_block_root(Into::<[u8; 32]>::into(reverted_block_root).pack())
        .rollup_config_hash(rollup_config_hash.pack())
        .status((Status::Running as u8).into())
        .version(2u8.into())
        .build();
    Ok(ProduceBlockResult {
        block,
        global_state,
        deposit_cells: deposits,
        withdrawal_extras: withdrawals,
    })
}

pub fn get_last_finalized_withdrawal(chain: &Chain) -> WithdrawalCursor {
    let last_global_state = chain.local_state().last_global_state();

    if last_global_state.version_u8() >= 2 {
        last_global_state.finalized_withdrawal_cursor()
    } else {
        // Upgrade to v2
        WithdrawalCursor::new_builder()
            .block_number(chain.local_state().tip().raw().number())
            .index(WithdrawalCursor::ALL_WITHDRAWALS.pack())
            .build()
    }
}

// Generate produce block param
#[instrument(skip_all, fields(mem_block = mem_block.block_info().number().unpack()))]
pub fn generate_produce_block_param(
    store: &Store,
    mem_block: MemBlock,
    post_merkle_state: AccountMerkleState,
    last_finalized_withdrawal: WithdrawalCursor,
) -> Result<BlockParam> {
    let db = store.begin_transaction();
    let tip_block_number = mem_block.block_info().number().unpack().saturating_sub(1);
    let tip_block_hash = {
        let opt = db.get_block_hash_by_number(tip_block_number)?;
        opt.ok_or_else(|| anyhow!("[produce block] tip block {} not found", tip_block_number))?
    };

    // generate kv state & merkle proof from tip state
    let chain_state = db.state_tree(StateContext::ReadOnly)?;

    let kv_state: Vec<(H256, H256)> = mem_block
        .touched_keys()
        .iter()
        .map(|k| {
            chain_state
                .get_raw(k)
                .map(|v| (*k, v))
                .map_err(|err| anyhow!("can't fetch value error: {:?}", err))
        })
        .collect::<Result<_>>()?;
    let kv_state_proof = if kv_state.is_empty() {
        // nothing need to prove
        Vec::new()
    } else {
        let account_smt = db.account_smt()?;

        account_smt
            .merkle_proof(kv_state.iter().map(|(k, _v)| *k).collect())
            .map_err(|err| anyhow!("merkle proof error: {:?}", err))?
            .compile(kv_state.clone())?
            .0
    };

    let txs: Vec<_> = mem_block
        .txs()
        .iter()
        .map(|tx_hash| {
            db.get_mem_pool_transaction(tx_hash)?
                .ok_or_else(|| anyhow!("can't find tx_hash from mem pool"))
        })
        .collect::<Result<_>>()?;
    let deposits: Vec<_> = mem_block.deposits().to_vec();
    let withdrawals: Vec<_> = mem_block
        .withdrawals()
        .iter()
        .map(|withdrawal_hash| {
            db.get_mem_pool_withdrawal(withdrawal_hash)?.ok_or_else(|| {
                anyhow!(
                    "can't find withdrawal_hash from mem pool {}",
                    hex::encode(withdrawal_hash.as_slice())
                )
            })
        })
        .collect::<Result<_>>()?;
    let state_checkpoint_list = mem_block.state_checkpoints().to_vec();
    let txs_prev_state_checkpoint = mem_block
        .txs_prev_state_checkpoint()
        .ok_or_else(|| anyhow!("Mem block has no txs prev state checkpoint"))?;
    let prev_merkle_state = mem_block.prev_merkle_state().clone();
    let parent_block = db
        .get_block(&tip_block_hash)?
        .ok_or_else(|| anyhow!("can't found tip block"))?;

    // check output block state consistent
    {
        let tip_block = db.get_last_valid_tip_block()?;
        assert_eq!(
            parent_block.hash(),
            tip_block.hash(),
            "check tip block consistent"
        );
        assert_eq!(
            prev_merkle_state,
            parent_block.raw().post_account(),
            "check mem block prev merkle state"
        );

        // check smt root
        let expected_kv_state_root: H256 = prev_merkle_state.merkle_root().unpack();
        let smt = db.account_smt()?;
        assert_eq!(
            smt.root(),
            &expected_kv_state_root,
            "check smt root consistent"
        );

        if !kv_state_proof.is_empty() {
            log::debug!("[output mem-block] check merkle proof");
            // check state merkle proof before output
            let prev_kv_state_root = CompiledMerkleProof(kv_state_proof.clone())
                .compute_root::<Blake2bHasher>(kv_state.clone())?;
            let expected_kv_state_root: H256 = prev_merkle_state.merkle_root().unpack();
            assert_eq!(
                expected_kv_state_root, prev_kv_state_root,
                "check state merkle proof"
            );
        }

        let tip_block_post_account = tip_block.raw().post_account();
        assert_eq!(
            prev_merkle_state, tip_block_post_account,
            "check output mem block txs prev state"
        );
        if withdrawals.is_empty() && deposits.is_empty() {
            let post_block_checkpoint = calculate_state_checkpoint(
                &tip_block_post_account.merkle_root().unpack(),
                tip_block_post_account.count().unpack(),
            );
            assert_eq!(
                txs_prev_state_checkpoint, post_block_checkpoint,
                "check mem block txs prev state"
            );
            if txs.is_empty() {
                assert_eq!(
                    post_merkle_state, tip_block_post_account,
                    "check mem block post account"
                )
            }
        }
    }

    let block_info = mem_block.block_info();
    let param = BlockParam {
        number: block_info.number().unpack(),
        block_producer: block_info.block_producer().unpack(),
        timestamp: block_info.timestamp().unpack(),
        txs,
        deposits,
        withdrawals,
        state_checkpoint_list,
        parent_block,
        txs_prev_state_checkpoint,
        prev_merkle_state,
        post_merkle_state,
        kv_state,
        kv_state_proof,
        last_finalized_withdrawal,
    };

    log::debug!(
        "output mem block, txs: {} tx withdrawals: {} state_checkpoints: {}",
        mem_block.txs().len(),
        mem_block.withdrawals().len(),
        mem_block.state_checkpoints().len(),
    );

    Ok(param)
}
