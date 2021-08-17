//! Block producer
//! Block producer assemble serveral Godwoken components into a single executor.
//! A block producer can act without the ability of produce block.

// FIXME:
use crate::challenger::offchain::{OffChainCancelChallengeValidator, OffChainContext};

use anyhow::{anyhow, Result};
use gw_common::{h256_ext::H256Ext, merkle_utils::calculate_merkle_root, smt::Blake2bHasher, H256};
use gw_generator::Generator;
use gw_store::transaction::StoreTransaction;
use gw_types::{
    core::Status,
    offchain::BlockParam,
    packed::{
        BlockMerkleState, GlobalState, L2Block, RawL2Block, SubmitTransactions, SubmitWithdrawals,
    },
    prelude::*,
};

pub struct ProduceBlockResult {
    pub block: L2Block,
    pub global_state: GlobalState,
}

pub struct ProduceBlockParam {
    pub stake_cell_owner_lock_hash: H256,
    pub reverted_block_root: H256,
    pub rollup_config_hash: H256,
    pub block_param: BlockParam,
    pub offchain_context: OffChainContext,
}

/// Produce block
/// this method take txs & withdrawal requests from tx pool and produce a new block
/// the package method should packs the items in order:
/// withdrawals, then deposits, finally the txs. Thus, the state-validator can verify this correctly
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
                block_producer_id,
                timestamp,
                txs,
                deposits: _,
                withdrawals,
                parent_block,
                prev_merkle_state,
                state_checkpoint_list,
                txs_prev_state_checkpoint,
                kv_state,
                kv_state_proof,
                post_merkle_state,
            },
        offchain_context,
    } = param;

    let rollup_context = generator.rollup_context();
    let parent_block_hash: H256 = parent_block.hash().into();

    // assemble block
    let submit_txs = {
        let tx_witness_root =
            calculate_merkle_root(txs.iter().map(|tx| tx.witness_hash().into()).collect())
                .map_err(|err| anyhow!("merkle root error: {:?}", err))?;
        let tx_count = txs.len() as u32;
        SubmitTransactions::new_builder()
            .tx_witness_root(tx_witness_root.pack())
            .tx_count(tx_count.pack())
            .prev_state_checkpoint(txs_prev_state_checkpoint.pack())
            .build()
    };
    let submit_withdrawals = {
        let withdrawal_witness_root = calculate_merkle_root(
            withdrawals
                .iter()
                .map(|request| request.witness_hash().into())
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
        .block_producer_id(block_producer_id.pack())
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
    let block = L2Block::new_builder()
        .raw(raw_block)
        .kv_state(packed_kv_state)
        .kv_state_proof(kv_state_proof.pack())
        .transactions(txs.pack())
        .withdrawals(withdrawals.pack())
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
        .last_finalized_block_number(last_finalized_block_number.pack())
        .reverted_block_root(Into::<[u8; 32]>::into(reverted_block_root).pack())
        .rollup_config_hash(rollup_config_hash.pack())
        .status((Status::Running as u8).into())
        .build();
    Ok(ProduceBlockResult {
        block,
        global_state,
    })
}
