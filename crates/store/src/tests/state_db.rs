use crate::{
    state_db::{StateDBTransaction, StateDBVersion},
    traits::KVStore,
    transaction::StoreTransaction,
    Store,
};
use gw_common::H256;
use gw_types::{
    packed::{GlobalState, L2Block, L2BlockCommittedInfo, L2Transaction, TxReceipt},
    prelude::*,
};

fn get_state_db_from_mock_data(
    db: &StoreTransaction,
    block_number: u64,
    tx_index: u32,
) -> StateDBTransaction {
    let version = StateDBVersion::from_genesis(); // just as a placeholder
    StateDBTransaction::from_tx_index(db, version, block_number, tx_index)
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
        )
        .unwrap();
    store_txn.commit().unwrap();

    let raw = block.raw();
    let block_number = raw.number();
    let block_hash = raw.hash();
    assert_eq!(0u64, block_number.unpack());

    let state_db_version = StateDBVersion::from_genesis();
    assert_eq!(true, state_db_version.is_genesis_version());
    let db = store.begin_transaction();
    let state_db = StateDBTransaction::from_version(&db, state_db_version);
    assert!(state_db.is_ok());

    let state_db_version = StateDBVersion::from_block_hash(block_hash.into());
    assert_eq!(false, state_db_version.is_genesis_version());
    let db = store.begin_transaction();
    let state_db = StateDBTransaction::from_version(&db, state_db_version);
    assert!(state_db.is_ok());

    let state_db_version = StateDBVersion::from_block_hash(H256::zero());
    assert_eq!(false, state_db_version.is_genesis_version());
    let db = store.begin_transaction();
    let state_db = StateDBTransaction::from_version(&db, state_db_version);
    assert_eq!(state_db.unwrap_err().message, "Block doesn't exist");

    let state_db_version = StateDBVersion::from_tx_index(block_hash.into(), 0u32);
    assert_eq!(false, state_db_version.is_genesis_version());
    let db = store.begin_transaction();
    let state_db = StateDBTransaction::from_version(&db, state_db_version);
    assert!(state_db.is_ok());

    let state_db_version = StateDBVersion::from_tx_index(block_hash.into(), 1u32);
    assert_eq!(false, state_db_version.is_genesis_version());
    let db = store.begin_transaction();
    let state_db = StateDBTransaction::from_version(&db, state_db_version);
    assert_eq!(state_db.unwrap_err().message, "Invalid tx index");
}

#[test]
fn construct_state_db_from_tx_index() {
    let store = Store::open_tmp().unwrap();
    let store_txn = store.begin_transaction();

    let block = L2Block::new_builder()
        .transactions(vec![L2Transaction::default(); 2].pack())
        .build();
    store_txn
        .insert_block(
            block.clone(),
            L2BlockCommittedInfo::default(),
            GlobalState::default(),
            vec![TxReceipt::default(); 2],
            Vec::new(),
        )
        .unwrap();
    store_txn.commit().unwrap();

    let raw = block.raw();
    let block_number = raw.number();
    let block_hash = raw.hash();
    assert_eq!(0u64, block_number.unpack());

    let state_db_version = StateDBVersion::from_tx_index(block_hash.into(), 0u32);
    assert_eq!(false, state_db_version.is_genesis_version());
    let db = store.begin_transaction();
    let state_db = StateDBTransaction::from_version(&db, state_db_version);
    assert!(state_db.is_ok());

    let state_db_version = StateDBVersion::from_tx_index(block_hash.into(), 1u32);
    assert_eq!(false, state_db_version.is_genesis_version());
    let db = store.begin_transaction();
    let state_db = StateDBTransaction::from_version(&db, state_db_version);
    assert!(state_db.is_ok());

    let state_db_version = StateDBVersion::from_tx_index(block_hash.into(), 2u32);
    assert_eq!(false, state_db_version.is_genesis_version());
    let db = store.begin_transaction();
    let state_db = StateDBTransaction::from_version(&db, state_db_version);
    assert_eq!(state_db.unwrap_err().message, "Invalid tx index");
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
