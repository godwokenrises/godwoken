use crate::{state::state_db::StateContext, traits::KVStore, transaction::StoreTransaction, Store};
use gw_common::{h256_ext::H256Ext, merkle_utils::calculate_state_checkpoint, state::State, H256};
use gw_db::schema::{
    Col, COLUMN_ACCOUNT_SMT_BRANCH, COLUMN_ACCOUNT_SMT_LEAF, COLUMN_BLOCK, COLUMN_SCRIPT,
};
use gw_types::{
    packed::{AccountMerkleState, L2Block, RawL2Block, SubmitTransactions},
    prelude::{Builder, Entity, Pack},
};

fn build_block(state: &impl State, block_number: u64, prev_txs_state_checkpoint: H256) -> L2Block {
    let post_state = AccountMerkleState::new_builder()
        .merkle_root(state.calculate_root().unwrap().pack())
        .count(state.get_account_count().unwrap().pack())
        .build();
    L2Block::new_builder()
        .raw(
            RawL2Block::new_builder()
                .number(block_number.pack())
                .post_account(post_state)
                .submit_transactions(
                    SubmitTransactions::new_builder()
                        .prev_state_checkpoint(prev_txs_state_checkpoint.pack())
                        .build(),
                )
                .build(),
        )
        .build()
}

fn commit_block(db: &StoreTransaction, block: L2Block) {
    let block_hash = block.hash();
    db.insert_raw(COLUMN_BLOCK, &block_hash, block.as_slice())
        .unwrap();
    db.attach_block(block).unwrap();
}

#[test]
fn test_state_with_version() {
    let store = Store::open_tmp().unwrap();
    let mut prev_txs_state_checkpoint = calculate_state_checkpoint(&H256::zero(), 0);
    // setup genesis block
    let genesis = L2Block::new_builder()
        .raw(
            RawL2Block::new_builder()
                .submit_transactions(
                    SubmitTransactions::new_builder()
                        .prev_state_checkpoint(prev_txs_state_checkpoint.pack())
                        .build(),
                )
                .build(),
        )
        .build();
    let db = store.begin_transaction();
    db.set_block_smt_root(H256::zero()).unwrap();
    commit_block(&db, genesis);
    db.commit().unwrap();

    // block 1
    {
        let db = store.begin_transaction();
        let mut state = db.state_tree(StateContext::AttachBlock(1)).unwrap();
        state
            .update_raw(H256::from_u32(1), H256::from_u32(2))
            .unwrap();
        state
            .update_raw(H256::from_u32(1), H256::from_u32(1))
            .unwrap();
        state
            .update_raw(H256::from_u32(2), H256::from_u32(3))
            .unwrap();
        state
            .update_raw(H256::from_u32(2), H256::from_u32(2))
            .unwrap();
        state
            .update_raw(H256::from_u32(3), H256::from_u32(3))
            .unwrap();
        state
            .update_raw(H256::from_u32(4), H256::from_u32(4))
            .unwrap();
        commit_block(&db, build_block(&state, 1, prev_txs_state_checkpoint));
        prev_txs_state_checkpoint = state.calculate_state_checkpoint().unwrap();
        db.commit().unwrap();
    }
    {
        let db = store.begin_transaction();
        let state = db.state_tree(StateContext::ReadOnly).unwrap();
        let v = state.get_raw(&H256::from_u32(1)).unwrap();
        assert_eq!(v, H256::from_u32(1));
        let v = state.get_raw(&H256::from_u32(2)).unwrap();
        assert_eq!(v, H256::from_u32(2));
        let v = state.get_raw(&H256::from_u32(3)).unwrap();
        assert_eq!(v, H256::from_u32(3));
    }

    // attach block 2
    {
        let db = store.begin_transaction();
        let mut state = db.state_tree(StateContext::AttachBlock(2)).unwrap();
        state
            .update_raw(H256::from_u32(1), H256::from_u32(1))
            .unwrap();
        state
            .update_raw(H256::from_u32(2), H256::from_u32(4))
            .unwrap();
        state.update_raw(H256::from_u32(4), H256::zero()).unwrap();
        state
            .update_raw(H256::from_u32(5), H256::from_u32(25))
            .unwrap();
        commit_block(&db, build_block(&state, 2, prev_txs_state_checkpoint));
        db.commit().unwrap();
    }
    {
        let db = store.begin_transaction();
        let state = db.state_tree(StateContext::ReadOnly).unwrap();
        let v = state.get_raw(&H256::from_u32(1)).unwrap();
        assert_eq!(v, H256::from_u32(1));
        let v = state.get_raw(&H256::from_u32(2)).unwrap();
        assert_eq!(v, H256::from_u32(4));
        let v = state.get_raw(&H256::from_u32(3)).unwrap();
        assert_eq!(v, H256::from_u32(3));
        let v = state.get_raw(&H256::from_u32(4)).unwrap();
        assert_eq!(v, H256::zero());
        let v = state.get_raw(&H256::from_u32(5)).unwrap();
        assert_eq!(v, H256::from_u32(25));
    }

    // detach block 2
    {
        let db = store.begin_transaction();
        db.detach_block(&db.get_tip_block().unwrap()).unwrap();
        let mut state = db.state_tree(StateContext::DetachBlock(2)).unwrap();
        state.detach_block_state().unwrap();
        prev_txs_state_checkpoint = state.calculate_state_checkpoint().unwrap();
        db.commit().unwrap();
    }
    {
        let db = store.begin_transaction();
        let state = db.state_tree(StateContext::ReadOnly).unwrap();
        let v = state.get_raw(&H256::from_u32(1)).unwrap();
        assert_eq!(v, H256::from_u32(1));
        let v = state.get_raw(&H256::from_u32(2)).unwrap();
        assert_eq!(v, H256::from_u32(2));
        let v = state.get_raw(&H256::from_u32(3)).unwrap();
        assert_eq!(v, H256::from_u32(3));
        let v = state.get_raw(&H256::from_u32(4)).unwrap();
        assert_eq!(v, H256::from_u32(4));
        let v = state.get_raw(&H256::from_u32(5)).unwrap();
        assert_eq!(v, H256::zero());
    }

    // attach 2 again
    {
        let db = store.begin_transaction();
        let mut state = db.state_tree(StateContext::AttachBlock(2)).unwrap();
        state
            .update_raw(H256::from_u32(1), H256::from_u32(1))
            .unwrap();
        state
            .update_raw(H256::from_u32(2), H256::from_u32(4))
            .unwrap();
        state.update_raw(H256::from_u32(4), H256::zero()).unwrap();
        state
            .update_raw(H256::from_u32(5), H256::from_u32(25))
            .unwrap();
        commit_block(&db, build_block(&state, 2, prev_txs_state_checkpoint));
        db.commit().unwrap();
    }
    {
        let db = store.begin_transaction();
        let state = db.state_tree(StateContext::ReadOnly).unwrap();
        let v = state.get_raw(&H256::from_u32(1)).unwrap();
        assert_eq!(v, H256::from_u32(1));
        let v = state.get_raw(&H256::from_u32(2)).unwrap();
        assert_eq!(v, H256::from_u32(4));
        let v = state.get_raw(&H256::from_u32(3)).unwrap();
        assert_eq!(v, H256::from_u32(3));
        let v = state.get_raw(&H256::from_u32(4)).unwrap();
        assert_eq!(v, H256::zero());
        let v = state.get_raw(&H256::from_u32(5)).unwrap();
        assert_eq!(v, H256::from_u32(25));
    }

    // check block 1
    {
        let db = store.begin_transaction();
        let state = db.state_tree(StateContext::ReadOnlyHistory(1)).unwrap();
        let v = state.get_raw(&H256::from_u32(1)).unwrap();
        assert_eq!(v, H256::from_u32(1));
        let v = state.get_raw(&H256::from_u32(2)).unwrap();
        assert_eq!(v, H256::from_u32(2));
        let v = state.get_raw(&H256::from_u32(3)).unwrap();
        assert_eq!(v, H256::from_u32(3));
        let v = state.get_raw(&H256::from_u32(4)).unwrap();
        assert_eq!(v, H256::from_u32(4));
        let v = state.get_raw(&H256::from_u32(5)).unwrap();
        assert_eq!(v, H256::zero());
    }
}
