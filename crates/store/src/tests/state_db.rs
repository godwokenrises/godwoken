use crate::{
    state_db::{StateDBTransaction, StateDBVersion},
    traits::KVStore,
    Store,
};
use gw_common::H256;

fn get_state_db_txn_from_tx_index(
    store: &Store,
    block_number: u64,
    tx_index: u32,
) -> StateDBTransaction {
    let store_txn = store.begin_transaction();
    StateDBTransaction::from_tx_index(store_txn, block_number, tx_index)
}

#[test]
fn construct_version() {
    let state_db_version = StateDBVersion::from_genesis();
    assert!(state_db_version.is_genesis_version());

    let state_db_version = StateDBVersion::from_block_hash([1u8; 32].into());
    assert!(!state_db_version.is_genesis_version());

    let state_db_version = StateDBVersion::from_tx_index([1u8; 32].into(), 100u32);
    assert!(!state_db_version.is_genesis_version());
}

#[test]
fn construct_state_db_txn_from_version() {
    let store = Store::open_tmp().unwrap();

    let version = StateDBVersion::from_genesis();
    assert!(store.state_at(version).is_ok());

    // This case will always be passed, for the db is empty.
    let version = StateDBVersion::from_block_hash(H256::zero());
    assert!(store.state_at(version).is_err());

    // This case will always be passed, for the db is empty.
    let version = StateDBVersion::from_tx_index(H256::zero(), 5u32);
    assert!(store.state_at(version).is_err());
}

#[test]
fn insert_and_get() {
    let store = Store::open_tmp().unwrap();

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 1u64, 1u32);
    state_db_txn.insert_raw("1", &[1, 1], &[1, 1, 1]).unwrap();
    state_db_txn.commit().unwrap();
    assert!(state_db_txn.get("1", &[1]).is_none());
    assert_eq!(
        vec![1, 1, 1].into_boxed_slice(),
        state_db_txn.get("1", &[1, 1]).unwrap()
    );

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 1u64, 2u32);
    assert_eq!(
        vec![1, 1, 1].into_boxed_slice(),
        state_db_txn.get("1", &[1, 1]).unwrap()
    );
    assert!(state_db_txn.get("1", &[1]).is_none());
    assert!(state_db_txn.get("1", &[2]).is_none());
    state_db_txn.insert_raw("1", &[2], &[2, 2, 2]).unwrap();
    state_db_txn.commit().unwrap();
    assert!(state_db_txn.get("1", &[1]).is_none());
    assert_eq!(
        vec![2, 2, 2].into_boxed_slice(),
        state_db_txn.get("1", &[2]).unwrap()
    );

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 1u64, 4u32);
    assert_eq!(
        vec![1, 1, 1].into_boxed_slice(),
        state_db_txn.get("1", &[1, 1]).unwrap()
    );
    assert_eq!(
        vec![2, 2, 2].into_boxed_slice(),
        state_db_txn.get("1", &[2]).unwrap()
    );
    state_db_txn.insert_raw("1", &[2], &[3, 3, 3]).unwrap();
    state_db_txn.commit().unwrap();
    assert_eq!(
        vec![3, 3, 3].into_boxed_slice(),
        state_db_txn.get("1", &[2]).unwrap()
    );

    // overwrite
    let state_db_txn = get_state_db_txn_from_tx_index(&store, 1u64, 4u32);
    state_db_txn.insert_raw("1", &[2], &[4, 4, 4]).unwrap();
    state_db_txn.commit().unwrap();
    assert_eq!(
        vec![4, 4, 4].into_boxed_slice(),
        state_db_txn.get("1", &[2]).unwrap()
    );

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 1u64, 2u32);
    assert_eq!(
        vec![2, 2, 2].into_boxed_slice(),
        state_db_txn.get("1", &[2]).unwrap()
    );

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 1u64, 5u32);
    assert_eq!(
        vec![4, 4, 4].into_boxed_slice(),
        state_db_txn.get("1", &[2]).unwrap()
    );
}

#[test]
fn insert_and_get_cross_block() {
    let store = Store::open_tmp().unwrap();

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 1u64, 1u32);
    state_db_txn.insert_raw("1", &[1, 1], &[1, 1, 1]).unwrap();
    state_db_txn.commit().unwrap();
    assert_eq!(
        vec![1, 1, 1].into_boxed_slice(),
        state_db_txn.get("1", &[1, 1]).unwrap()
    );

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 2u64, 5u32);
    assert_eq!(
        vec![1, 1, 1].into_boxed_slice(),
        state_db_txn.get("1", &[1, 1]).unwrap()
    );
    state_db_txn.insert_raw("1", &[2], &[2, 2, 2]).unwrap();
    state_db_txn.commit().unwrap();
    assert_eq!(
        vec![2, 2, 2].into_boxed_slice(),
        state_db_txn.get("1", &[2]).unwrap()
    );

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 3u64, 1u32);
    assert_eq!(
        vec![1, 1, 1].into_boxed_slice(),
        state_db_txn.get("1", &[1, 1]).unwrap()
    );
    assert_eq!(
        vec![2, 2, 2].into_boxed_slice(),
        state_db_txn.get("1", &[2]).unwrap()
    );
    state_db_txn.insert_raw("1", &[2], &[3, 3, 3]).unwrap();
    state_db_txn.commit().unwrap();
    assert_eq!(
        vec![3, 3, 3].into_boxed_slice(),
        state_db_txn.get("1", &[2]).unwrap()
    );

    // overwrite
    let state_db_txn = get_state_db_txn_from_tx_index(&store, 2u64, 5u32);
    state_db_txn.insert_raw("1", &[2], &[0, 2, 2]).unwrap();
    state_db_txn.commit().unwrap();
    assert_eq!(
        vec![0, 2, 2].into_boxed_slice(),
        state_db_txn.get("1", &[2]).unwrap()
    );

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 2u64, 4u32);
    assert!(state_db_txn.get("1", &[2]).is_none());

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 2u64, 6u32);
    assert_eq!(
        vec![0, 2, 2].into_boxed_slice(),
        state_db_txn.get("1", &[2]).unwrap()
    );

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 3u64, 2u32);
    assert_eq!(
        vec![3, 3, 3].into_boxed_slice(),
        state_db_txn.get("1", &[2]).unwrap()
    );
}

#[test]
fn insert_keys_with_the_same_version() {
    let store = Store::open_tmp().unwrap();

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 2u64, 5u32);
    state_db_txn
        .insert_raw("0", &[1, 1], &[1, 1, 1, 1])
        .unwrap();
    state_db_txn.commit().unwrap();

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 2u64, 5u32);
    state_db_txn.insert_raw("0", &[2], &[2, 2, 2]).unwrap();
    state_db_txn.commit().unwrap();

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 2u64, 5u32);
    state_db_txn.insert_raw("1", &[1, 1], &[1, 1, 1]).unwrap();
    state_db_txn.commit().unwrap();

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 2u64, 5u32);
    state_db_txn.insert_raw("1", &[2], &[2, 2]).unwrap();
    state_db_txn.commit().unwrap();

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 2u64, 4u32);
    assert!(state_db_txn.get("1", &[1, 1]).is_none());
    assert!(state_db_txn.get("1", &[2]).is_none());

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 2u64, 5u32);
    assert_eq!(
        vec![1, 1, 1].into_boxed_slice(),
        state_db_txn.get("1", &[1, 1]).unwrap()
    );
    assert_eq!(
        vec![2, 2].into_boxed_slice(),
        state_db_txn.get("1", &[2]).unwrap()
    );

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 2u64, 6u32);
    assert_eq!(
        vec![1, 1, 1].into_boxed_slice(),
        state_db_txn.get("1", &[1, 1]).unwrap()
    );
    assert_eq!(
        vec![2, 2].into_boxed_slice(),
        state_db_txn.get("1", &[2]).unwrap()
    );

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 3u64, 6u32);
    assert_eq!(
        vec![1, 1, 1].into_boxed_slice(),
        state_db_txn.get("1", &[1, 1]).unwrap()
    );
    assert_eq!(
        vec![2, 2].into_boxed_slice(),
        state_db_txn.get("1", &[2]).unwrap()
    );
}

#[test]
fn delete() {
    let store = Store::open_tmp().unwrap();

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 1u64, 1u32);
    state_db_txn.insert_raw("1", &[2], &[1, 1, 1]).unwrap();
    state_db_txn.commit().unwrap();
    assert_eq!(
        vec![1, 1, 1].into_boxed_slice(),
        state_db_txn.get("1", &[2]).unwrap()
    );

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 2u64, 2u32);
    assert_eq!(
        vec![1, 1, 1].into_boxed_slice(),
        state_db_txn.get("1", &[2]).unwrap()
    );
    state_db_txn.insert_raw("1", &[2], &[2, 2, 2]).unwrap();
    state_db_txn.commit().unwrap();
    assert_eq!(
        vec![2, 2, 2].into_boxed_slice(),
        state_db_txn.get("1", &[2]).unwrap()
    );

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 2u64, 4u32);
    state_db_txn.delete("1", &[2]).unwrap();
    state_db_txn.commit().unwrap();
    assert!(state_db_txn.get("1", &[2]).is_none());

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 2u64, 5u32);
    assert!(state_db_txn.get("1", &[2]).is_none());

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 3u64, 1u32);
    assert!(state_db_txn.get("1", &[2]).is_none());

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 2u64, 3u32);
    assert_eq!(
        vec![2, 2, 2].into_boxed_slice(),
        state_db_txn.get("1", &[2]).unwrap()
    );

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 4u64, 1u32);
    state_db_txn
        .insert_raw("1", &[2], &0u32.to_be_bytes())
        .unwrap();
    state_db_txn.commit().unwrap();
    assert_eq!(
        Box::<[u8]>::from(&0u32.to_be_bytes()[..]),
        state_db_txn.get("1", &[2]).unwrap()
    );
}

#[test]
#[should_panic]
fn insert_special_value_0u8() {
    let store = Store::open_tmp().unwrap();

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 4u64, 1u32);

    // insert 0u8 is a special case.
    // value is 0u8 presents the key has been deleted.
    // so make sure DO NOT insert 0u8 as value by user.
    // 0u16, 0u32, 0u64 as value are even allowed.
    state_db_txn
        .insert_raw("1", &[2], &0u8.to_be_bytes())
        .unwrap();
}
