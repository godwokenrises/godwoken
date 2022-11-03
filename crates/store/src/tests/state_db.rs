use crate::{
    state::{history::history_state::RWConfig, traits::JournalDB, BlockStateDB},
    traits::{chain_store::ChainStore, kv_store::KVStoreWrite},
    transaction::StoreTransaction,
    Store,
};
use gw_common::{h256_ext::H256Ext, merkle_utils::calculate_state_checkpoint, state::State, H256};
use gw_db::schema::COLUMN_BLOCK;
use gw_types::{
    packed::{
        AccountMerkleState, L2Block, NumberHash, RawL2Block, SubmitTransactions, Transaction,
    },
    prelude::{Builder, Entity, Pack, Unpack},
};

fn build_block<S: State + JournalDB>(
    state: &mut S,
    block_number: u64,
    prev_txs_state_checkpoint: H256,
) -> L2Block {
    state.finalise().unwrap();
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
        let mut state = BlockStateDB::from_store(&db, RWConfig::attach_block(1)).unwrap();
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
        commit_block(&db, build_block(&mut state, 1, prev_txs_state_checkpoint));
        prev_txs_state_checkpoint = state.calculate_state_checkpoint().unwrap();
        db.set_block_submit_tx(1, &Transaction::default().as_reader())
            .unwrap();
        db.set_last_submitted_block_number_hash(
            &NumberHash::new_builder()
                .number(1.pack())
                .build()
                .as_reader(),
        )
        .unwrap();
        db.commit().unwrap();
    }
    {
        let db = &store.begin_transaction();
        let state = BlockStateDB::from_store(db, RWConfig::readonly()).unwrap();
        let v = state.get_raw(&H256::from_u32(1)).unwrap();
        assert_eq!(v, H256::from_u32(1));
        let v = state.get_raw(&H256::from_u32(2)).unwrap();
        assert_eq!(v, H256::from_u32(2));
        let v = state.get_raw(&H256::from_u32(3)).unwrap();
        assert_eq!(v, H256::from_u32(3));
        assert_eq!(
            db.get_last_submitted_block_number_hash()
                .map(|nh| nh.number().unpack()),
            Some(1),
        );
        assert!(db.get_block_submit_tx(1).is_some());
    }

    // attach block 2
    {
        let db = &store.begin_transaction();
        let mut state = BlockStateDB::from_store(db, RWConfig::attach_block(2)).unwrap();
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
        commit_block(db, build_block(&mut state, 2, prev_txs_state_checkpoint));
        db.set_last_confirmed_block_number_hash(
            &NumberHash::new_builder()
                .number(2.pack())
                .build()
                .as_reader(),
        )
        .unwrap();
        assert_eq!(
            db.get_last_confirmed_block_number_hash()
                .map(|nh| nh.number().unpack()),
            Some(2)
        );
        db.commit().unwrap();
    }
    {
        let db = store.begin_transaction();
        let state = BlockStateDB::from_store(&db, RWConfig::readonly()).unwrap();
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
        let db = &store.begin_transaction();
        db.detach_block(&db.get_tip_block().unwrap()).unwrap();
        let mut state = BlockStateDB::from_store(db, RWConfig::detach_block()).unwrap();
        state.detach_block_state(2).unwrap();
        prev_txs_state_checkpoint = state.calculate_state_checkpoint().unwrap();
        db.commit().unwrap();
    }
    {
        let db = &store.begin_transaction();
        let state = BlockStateDB::from_store(db, RWConfig::readonly()).unwrap();
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
        assert_eq!(
            db.get_last_confirmed_block_number_hash()
                .map(|nh| nh.number().unpack()),
            Some(1)
        );
    }

    // attach 2 again
    {
        let db = store.begin_transaction();
        let mut state = BlockStateDB::from_store(&db, RWConfig::attach_block(2)).unwrap();
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
        commit_block(&db, build_block(&mut state, 2, prev_txs_state_checkpoint));
        db.commit().unwrap();
    }
    {
        let db = store.begin_transaction();
        let state = BlockStateDB::from_store(&db, RWConfig::readonly()).unwrap();
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
        let state = BlockStateDB::from_store(&db, RWConfig::history_block(1)).unwrap();
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
