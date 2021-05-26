use crate::{
    state_db::{CheckPoint, StateDBMode, StateDBTransaction, SubState, WriteContext},
    traits::KVStore,
    transaction::StoreTransaction,
    Store,
};
use gw_common::{merkle_utils::calculate_state_checkpoint, H256};
use gw_types::{
    packed::{
        AccountMerkleState, Byte32, GlobalState, L2Block, L2BlockCommittedInfo, L2Transaction,
        RawL2Block, TxReceipt, WithdrawalRequest,
    },
    prelude::*,
};

fn get_state_db_from_mock_data(
    db: &StoreTransaction,
    block_number: u64,
    tx_index: u32,
) -> StateDBTransaction {
    let checkpoint = CheckPoint::new(block_number, SubState::Tx(tx_index));
    StateDBTransaction::from_checkpoint(db, checkpoint, StateDBMode::Write(WriteContext::new(0)))
        .unwrap()
}

#[test]
fn construct_state_db_from_block_hash() {
    let store = Store::open_tmp().unwrap();
    let store_txn = store.begin_transaction();

    let block = L2Block::default();
    store_txn
        .insert_block(
            block.clone(),
            L2BlockCommittedInfo::default(),
            GlobalState::default(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
    store_txn.commit().unwrap();

    let raw = block.raw();
    let block_number = raw.number().unpack();
    let block_hash = raw.hash();
    assert_eq!(0u64, block_number);

    let state_checkpoint = CheckPoint::from_genesis();
    let db = store.begin_transaction();
    let state_db = StateDBTransaction::from_checkpoint(&db, state_checkpoint, StateDBMode::Genesis);
    assert!(state_db.is_ok());
    assert_eq!(true, state_db.unwrap().mode() == StateDBMode::Genesis);

    let state_checkpoint =
        CheckPoint::from_block_hash(&db, block_hash.into(), SubState::Block).unwrap();
    let db = store.begin_transaction();
    let state_db =
        StateDBTransaction::from_checkpoint(&db, state_checkpoint, StateDBMode::ReadOnly);
    assert!(state_db.is_ok());
    assert_eq!(false, state_db.unwrap().mode() == StateDBMode::Genesis);

    let state_checkpoint = CheckPoint::from_block_hash(&db, H256::zero(), SubState::Block);
    assert_eq!(
        state_checkpoint.unwrap_err().to_string(),
        "block isn't exist".to_string()
    );

    let state_checkpoint = CheckPoint::new(block_number.into(), SubState::Tx(0u32));
    let db = store.begin_transaction();
    let state_db = StateDBTransaction::from_checkpoint(
        &db,
        state_checkpoint,
        StateDBMode::Write(WriteContext::new(0)),
    );
    assert!(state_db.is_ok());
    assert_eq!(false, state_db.unwrap().mode() == StateDBMode::Genesis);

    let state_checkpoint = CheckPoint::from_block_hash(&db, block_hash.into(), SubState::Tx(1u32));
    assert_eq!(
        state_checkpoint.unwrap_err().to_string(),
        "invalid tx substate index"
    );

    let state_checkpoint =
        CheckPoint::from_block_hash(&db, block_hash.into(), SubState::Withdrawal(1u32));
    assert_eq!(
        state_checkpoint.unwrap_err().to_string(),
        "invalid withdrawal substate index"
    );
}

#[test]
fn construct_state_db_from_sub_state() {
    let store = Store::open_tmp().unwrap();
    let store_txn = store.begin_transaction();

    let default_state_checkpoint: Byte32 = {
        let post_state = AccountMerkleState::default();
        let root: [u8; 32] = post_state.merkle_root().unpack();
        let checkpoint: [u8; 32] =
            calculate_state_checkpoint(&root.into(), post_state.count().unpack()).into();
        checkpoint.pack()
    };

    let raw_block = RawL2Block::new_builder()
        .state_checkpoint_list(vec![default_state_checkpoint; 5].pack())
        .build();

    let block = L2Block::new_builder()
        .transactions(vec![L2Transaction::default(); 2].pack())
        .withdrawals(vec![WithdrawalRequest::default(); 3].pack())
        .raw(raw_block)
        .build();
    store_txn
        .insert_block(
            block.clone(),
            L2BlockCommittedInfo::default(),
            GlobalState::default(),
            vec![TxReceipt::default(); 2],
            vec![AccountMerkleState::default(); 5],
            Vec::new(),
        )
        .unwrap();
    store_txn.commit().unwrap();

    let raw = block.raw();
    let block_number = raw.number().unpack();
    let block_hash = raw.hash();
    assert_eq!(0u64, block_number);

    let state_checkpoint = CheckPoint::new(block_number.into(), SubState::Tx(0u32));
    let db = store.begin_transaction();
    let state_db = StateDBTransaction::from_checkpoint(
        &db,
        state_checkpoint,
        StateDBMode::Write(WriteContext::new(3)),
    );
    assert!(state_db.is_ok());
    assert_eq!(false, state_db.unwrap().mode() == StateDBMode::Genesis);

    let state_checkpoint = CheckPoint::new(block_number.into(), SubState::Tx(1u32));
    let db = store.begin_transaction();
    let state_db = StateDBTransaction::from_checkpoint(
        &db,
        state_checkpoint,
        StateDBMode::Write(WriteContext::new(3)),
    );
    assert!(state_db.is_ok());
    assert_eq!(false, state_db.unwrap().mode() == StateDBMode::Genesis);

    let state_checkpoint = CheckPoint::new(block_number.into(), SubState::Withdrawal(1u32));
    let db = store.begin_transaction();
    let state_db =
        StateDBTransaction::from_checkpoint(&db, state_checkpoint, StateDBMode::ReadOnly);
    assert!(state_db.is_ok());
    assert_eq!(false, state_db.unwrap().mode() == StateDBMode::Genesis);

    let state_checkpoint =
        CheckPoint::from_block_hash(&db, block_hash.into(), SubState::Withdrawal(3u32));
    assert_eq!(
        state_checkpoint.unwrap_err().to_string(),
        "invalid withdrawal substate index"
    );

    let state_checkpoint = CheckPoint::from_block_hash(&db, block_hash.into(), SubState::Tx(2u32));
    assert_eq!(
        state_checkpoint.unwrap_err().to_string(),
        "invalid tx substate index"
    );
}

#[test]
fn commit_on_readonly_mode() {
    let store = Store::open_tmp().unwrap();
    let store_txn = store.begin_transaction();

    let block = L2Block::default();
    store_txn
        .insert_block(
            block.clone(),
            L2BlockCommittedInfo::default(),
            GlobalState::default(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
    store_txn.commit().unwrap();

    let state_checkpoint = CheckPoint::new(block.raw().number().unpack(), SubState::Block);
    let db = store.begin_transaction();
    let state_db =
        StateDBTransaction::from_checkpoint(&db, state_checkpoint, StateDBMode::ReadOnly).unwrap();
    assert_eq!(
        state_db.commit().unwrap_err().to_string(),
        "DB error commit on ReadOnly mode"
    );
}

#[test]
fn insert_and_get() {
    let store = Store::open_tmp().unwrap();

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 1u64, 1u32);
        state_db_txn.insert_raw(1, &[1, 1], &[1, 1, 1]).unwrap();
        state_db_txn.commit().unwrap();
        assert!(state_db_txn.get(1, &[1]).is_none());
        assert_eq!(
            vec![1, 1, 1].into_boxed_slice(),
            state_db_txn.get(1, &[1, 1]).unwrap()
        );
    }

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 1u64, 2u32);
        assert_eq!(
            vec![1, 1, 1].into_boxed_slice(),
            state_db_txn.get(1, &[1, 1]).unwrap()
        );
        assert!(state_db_txn.get(1, &[1]).is_none());
        assert!(state_db_txn.get(1, &[2]).is_none());
        state_db_txn.insert_raw(1, &[2], &[2, 2, 2]).unwrap();
        state_db_txn.commit().unwrap();
        assert!(state_db_txn.get(1, &[1]).is_none());
        assert_eq!(
            vec![2, 2, 2].into_boxed_slice(),
            state_db_txn.get(1, &[2]).unwrap()
        );
    }

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 1u64, 4u32);
        assert_eq!(
            vec![1, 1, 1].into_boxed_slice(),
            state_db_txn.get(1, &[1, 1]).unwrap()
        );
        assert_eq!(
            vec![2, 2, 2].into_boxed_slice(),
            state_db_txn.get(1, &[2]).unwrap()
        );
        state_db_txn.insert_raw(1, &[2], &[3, 3, 3]).unwrap();
        state_db_txn.commit().unwrap();
        assert_eq!(
            vec![3, 3, 3].into_boxed_slice(),
            state_db_txn.get(1, &[2]).unwrap()
        );
    }

    // overwrite
    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 1u64, 4u32);
        state_db_txn.insert_raw(1, &[2], &[4, 4, 4]).unwrap();
        state_db_txn.commit().unwrap();
        assert_eq!(
            vec![4, 4, 4].into_boxed_slice(),
            state_db_txn.get(1, &[2]).unwrap()
        );
    }

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 1u64, 2u32);
        assert_eq!(
            vec![2, 2, 2].into_boxed_slice(),
            state_db_txn.get(1, &[2]).unwrap()
        );
    }

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 1u64, 5u32);
        assert_eq!(
            vec![4, 4, 4].into_boxed_slice(),
            state_db_txn.get(1, &[2]).unwrap()
        );
    }
}

#[test]
fn insert_and_get_cross_block() {
    let store = Store::open_tmp().unwrap();

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 1u64, 1u32);
        state_db_txn.insert_raw(1, &[1, 1], &[1, 1, 1]).unwrap();
        state_db_txn.commit().unwrap();
        assert_eq!(
            vec![1, 1, 1].into_boxed_slice(),
            state_db_txn.get(1, &[1, 1]).unwrap()
        );
    }

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 2u64, 5u32);
        assert_eq!(
            vec![1, 1, 1].into_boxed_slice(),
            state_db_txn.get(1, &[1, 1]).unwrap()
        );
        state_db_txn.insert_raw(1, &[2], &[2, 2, 2]).unwrap();
        state_db_txn.commit().unwrap();
        assert_eq!(
            vec![2, 2, 2].into_boxed_slice(),
            state_db_txn.get(1, &[2]).unwrap()
        );
    }

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 3u64, 1u32);
        assert_eq!(
            vec![1, 1, 1].into_boxed_slice(),
            state_db_txn.get(1, &[1, 1]).unwrap()
        );
        assert_eq!(
            vec![2, 2, 2].into_boxed_slice(),
            state_db_txn.get(1, &[2]).unwrap()
        );
        state_db_txn.insert_raw(1, &[2], &[3, 3, 3]).unwrap();
        state_db_txn.commit().unwrap();
        assert_eq!(
            vec![3, 3, 3].into_boxed_slice(),
            state_db_txn.get(1, &[2]).unwrap()
        );
    }

    // overwrite
    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 2u64, 5u32);
        state_db_txn.insert_raw(1, &[2], &[0, 2, 2]).unwrap();
        state_db_txn.commit().unwrap();
        assert_eq!(
            vec![0, 2, 2].into_boxed_slice(),
            state_db_txn.get(1, &[2]).unwrap()
        );
    }

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 2u64, 4u32);
        assert!(state_db_txn.get(1, &[2]).is_none());
    }

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 2u64, 6u32);
        assert_eq!(
            vec![0, 2, 2].into_boxed_slice(),
            state_db_txn.get(1, &[2]).unwrap()
        );
    }

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 3u64, 2u32);
        assert_eq!(
            vec![3, 3, 3].into_boxed_slice(),
            state_db_txn.get(1, &[2]).unwrap()
        );
    }
}

#[test]
fn insert_keys_with_the_same_version() {
    let store = Store::open_tmp().unwrap();

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 2u64, 5u32);
        state_db_txn.insert_raw(0, &[1, 1], &[1, 1, 1, 1]).unwrap();
        state_db_txn.commit().unwrap();
    }

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 2u64, 5u32);
        state_db_txn.insert_raw(0, &[2], &[2, 2, 2]).unwrap();
        state_db_txn.commit().unwrap();
    }

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 2u64, 5u32);
        state_db_txn.insert_raw(1, &[1, 1], &[1, 1, 1]).unwrap();
        state_db_txn.commit().unwrap();
    }

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 2u64, 5u32);
        state_db_txn.insert_raw(1, &[2], &[2, 2]).unwrap();
        state_db_txn.commit().unwrap();
    }

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 2u64, 4u32);
        assert!(state_db_txn.get(1, &[1, 1]).is_none());
        assert!(state_db_txn.get(1, &[2]).is_none());
    }

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 2u64, 5u32);
        assert_eq!(
            vec![1, 1, 1].into_boxed_slice(),
            state_db_txn.get(1, &[1, 1]).unwrap()
        );
        assert_eq!(
            vec![2, 2].into_boxed_slice(),
            state_db_txn.get(1, &[2]).unwrap()
        );
    }

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 2u64, 6u32);
        assert_eq!(
            vec![1, 1, 1].into_boxed_slice(),
            state_db_txn.get(1, &[1, 1]).unwrap()
        );
        assert_eq!(
            vec![2, 2].into_boxed_slice(),
            state_db_txn.get(1, &[2]).unwrap()
        );
    }

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 3u64, 6u32);
        assert_eq!(
            vec![1, 1, 1].into_boxed_slice(),
            state_db_txn.get(1, &[1, 1]).unwrap()
        );
        assert_eq!(
            vec![2, 2].into_boxed_slice(),
            state_db_txn.get(1, &[2]).unwrap()
        );
    }
}

#[test]
fn delete() {
    let store = Store::open_tmp().unwrap();

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 1u64, 1u32);
        state_db_txn.insert_raw(1, &[2], &[1, 1, 1]).unwrap();
        state_db_txn.commit().unwrap();
        assert_eq!(
            vec![1, 1, 1].into_boxed_slice(),
            state_db_txn.get(1, &[2]).unwrap()
        );
    }

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 2u64, 2u32);
        assert_eq!(
            vec![1, 1, 1].into_boxed_slice(),
            state_db_txn.get(1, &[2]).unwrap()
        );
        state_db_txn.insert_raw(1, &[2], &[2, 2, 2]).unwrap();
        state_db_txn.commit().unwrap();
        assert_eq!(
            vec![2, 2, 2].into_boxed_slice(),
            state_db_txn.get(1, &[2]).unwrap()
        );
    }

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 2u64, 4u32);
        state_db_txn.delete(1, &[2]).unwrap();
        state_db_txn.commit().unwrap();
        assert!(state_db_txn.get(1, &[2]).is_none());
    }

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 2u64, 5u32);
        assert!(state_db_txn.get(1, &[2]).is_none());
    }

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 3u64, 1u32);
        assert!(state_db_txn.get(1, &[2]).is_none());
    }

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 2u64, 3u32);
        assert_eq!(
            vec![2, 2, 2].into_boxed_slice(),
            state_db_txn.get(1, &[2]).unwrap()
        );
    }

    {
        let db = store.begin_transaction();
        let state_db_txn = get_state_db_from_mock_data(&db, 4u64, 1u32);
        state_db_txn
            .insert_raw(1, &[2], &0u32.to_be_bytes())
            .unwrap();
        state_db_txn.commit().unwrap();
        assert_eq!(
            Box::<[u8]>::from(&0u32.to_be_bytes()[..]),
            state_db_txn.get(1, &[2]).unwrap()
        );
    }
}

#[test]
#[should_panic]
fn insert_special_value_0u8() {
    let store = Store::open_tmp().unwrap();

    let db = store.begin_transaction();
    let state_db_txn = get_state_db_from_mock_data(&db, 4u64, 1u32);

    // insert 0u8 is a special case.
    // value is 0u8 presents the key has been deleted.
    // so make sure DO NOT insert 0u8 as value by user.
    // 0u16, 0u32, 0u64 as value are even allowed.
    state_db_txn
        .insert_raw(1, &[2], &0u8.to_be_bytes())
        .unwrap();
}
