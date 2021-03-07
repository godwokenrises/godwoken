use gw_store::{Store, 
    state_db::{
        StateDBTransaction, 
        StateDBVersion,
    },
    traits::KVStore,
};
use gw_common::{H256};
use gw_db::{IteratorMode};

use std::collections::HashMap;

fn get_state_db_txn_from_version(store: &Store, block_ver: H256) -> StateDBTransaction {
    let store_txn = store.begin_transaction();
    let version = StateDBVersion::from_block_hash(block_ver);
    StateDBTransaction::from_version(store_txn, version)
}

fn get_state_db_txn_from_tx_index(store: &Store, block_number: u64, tx_index: u32) -> StateDBTransaction {
    let store_txn = store.begin_transaction();
    StateDBTransaction::from_tx_index(store_txn, block_number, tx_index)
}

#[test]
fn get_version() {
    let genesis_ver = StateDBVersion::from_genesis();
    let block_ver = StateDBVersion::from_block_hash([1;32].into());
    assert_eq!(genesis_ver.get_block_hash(), [1;32].into()); // TODO: solve to get the genesis block hash
    assert_eq!(block_ver.get_block_hash(), [1;32].into());
}

#[test]
fn get_state_db_txn() {
    let store = Store::open_tmp().unwrap();
    let _state_db_txn = get_state_db_txn_from_version(&store, H256::zero());
    // no panic
}

#[test]
fn insert_and_get() {
    let store = Store::open_tmp().unwrap();

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 1u64, 1u32);
    state_db_txn.insert_raw("1", &[1, 1], &[1, 1, 1]).unwrap();
    state_db_txn.commit().unwrap();
    assert_eq!(vec![1, 1, 1].as_slice(), state_db_txn.get("1", &[1, 1]).unwrap().as_ref());

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 1u64, 2u32);
    assert_eq!(vec![1, 1, 1].as_slice(), state_db_txn.get("1", &[1, 1]).unwrap().as_ref());
    assert!(state_db_txn.get("1", &[2]).is_none());
    state_db_txn.insert_raw("1", &[2], &[2, 2, 2]).unwrap();
    state_db_txn.commit().unwrap();
    assert_eq!(vec![2, 2, 2].as_slice(), state_db_txn.get("1", &[2]).unwrap().as_ref());

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 1u64, 4u32);
    state_db_txn.insert_raw("1", &[2], &[3, 3, 3]).unwrap();
    state_db_txn.commit().unwrap();
    assert_eq!(vec![3, 3, 3].as_slice(), state_db_txn.get("1", &[2]).unwrap().as_ref());

    // overwrite
    let state_db_txn = get_state_db_txn_from_tx_index(&store, 1u64, 2u32);
    state_db_txn.insert_raw("1", &[2], &[0, 2, 2]).unwrap();
    state_db_txn.commit().unwrap();
    assert_eq!(vec![0, 2, 2].as_slice(), state_db_txn.get("1", &[2]).unwrap().as_ref());

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 1u64, 3u32);
    assert_eq!(vec![0, 2, 2].as_slice(), state_db_txn.get("1", &[2]).unwrap().as_ref());
}

#[test]
fn insert_and_get_cross_block() {
    let store = Store::open_tmp().unwrap();

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 1u64, 1u32);
    state_db_txn.insert_raw("1", &[1, 1], &[1, 1, 1]).unwrap();
    state_db_txn.commit().unwrap();
    assert_eq!(vec![1, 1, 1].as_slice(), state_db_txn.get("1", &[1, 1]).unwrap().as_ref());

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 2u64, 5u32);
    assert_eq!(vec![1, 1, 1].as_slice(), state_db_txn.get("1", &[1, 1]).unwrap().as_ref());
    state_db_txn.insert_raw("1", &[2], &[2, 2, 2]).unwrap();
    state_db_txn.commit().unwrap();
    assert_eq!(vec![2, 2, 2].as_slice(), state_db_txn.get("1", &[2]).unwrap().as_ref());

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 3u64, 1u32);
    assert_eq!(vec![2, 2, 2].as_slice(), state_db_txn.get("1", &[1, 1]).unwrap().as_ref());
    state_db_txn.insert_raw("1", &[2], &[3, 3, 3]).unwrap();
    state_db_txn.commit().unwrap();
    assert_eq!(vec![3, 3, 3].as_slice(), state_db_txn.get("1", &[2]).unwrap().as_ref());

    // overwrite
    let state_db_txn = get_state_db_txn_from_tx_index(&store, 2u64, 5u32);
    state_db_txn.insert_raw("1", &[2], &[0, 2, 2]).unwrap();
    state_db_txn.commit().unwrap();
    assert_eq!(vec![0, 2, 2].as_slice(), state_db_txn.get("1", &[2]).unwrap().as_ref());

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 2u64, 6u32);
    assert_eq!(vec![0, 2, 2].as_slice(), state_db_txn.get("1", &[2]).unwrap().as_ref());
}

#[test]
fn get_iter() {
    let store = Store::open_tmp().unwrap();

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 1u64, 1u32);
    state_db_txn.insert_raw("1", &[1, 1], &[1, 1, 1]).unwrap();
    state_db_txn.commit().unwrap();

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 1u64, 2u32);
    state_db_txn.insert_raw("1", &[2], &[2, 2, 2]).unwrap();
    state_db_txn.commit().unwrap();

    let iter = state_db_txn.get_iter("1", IteratorMode::Start);
    let mut r = HashMap::new();
    for (key, val) in iter {
        r.insert(key.to_vec(), val.to_vec());
    }
    assert_eq!(2, r.len());
    assert_eq!(Some(&vec![1, 1, 1]), r.get(&vec![1, 1]));
    assert_eq!(Some(&vec![2, 2, 2]), r.get(&vec![2]));

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 1u64, 3u32);
    let iter = state_db_txn.get_iter("1", IteratorMode::Start);
    let mut r = HashMap::new();
    for (key, val) in iter {
        r.insert(key.to_vec(), val.to_vec());
    }
    assert_eq!(2, r.len());
    assert_eq!(Some(&vec![1, 1, 1]), r.get(&vec![1, 1]));
    assert_eq!(Some(&vec![2, 2, 2]), r.get(&vec![2]));
}

#[test]
fn delete() {
    let store = Store::open_tmp().unwrap();

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 1u64, 1u32);
    state_db_txn.insert_raw("1", &[2], &[1, 1, 1]).unwrap();
    state_db_txn.commit().unwrap();
    assert_eq!(vec![1, 1, 1].as_slice(), state_db_txn.get("1", &[2]).unwrap().as_ref());

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 2u64, 2u32);
    assert_eq!(vec![1, 1, 1].as_slice(), state_db_txn.get("1", &[2]).unwrap().as_ref());
    state_db_txn.insert_raw("1", &[2], &[2, 2, 2]).unwrap();
    state_db_txn.commit().unwrap();
    assert_eq!(vec![2, 2, 2].as_slice(), state_db_txn.get("1", &[2]).unwrap().as_ref());

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 2u64, 4u32);
    state_db_txn.delete("1", &[2]).unwrap();
    state_db_txn.commit().unwrap();
    assert!(state_db_txn.get("1", &[2]).is_none());

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 2u64, 5u32);
    assert!(state_db_txn.get("1", &[2]).is_none());

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 3u64, 1u32);
    assert!(state_db_txn.get("1", &[2]).is_none());

    let state_db_txn = get_state_db_txn_from_tx_index(&store, 2u64, 3u32);
    assert_eq!(vec![2, 2, 2].as_slice(), state_db_txn.get("1", &[2]).unwrap().as_ref());
}
