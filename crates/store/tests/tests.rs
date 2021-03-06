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

fn get_state_db_txn(store: &Store, block_ver: H256) -> StateDBTransaction {
    let store_txn = store.begin_transaction();
    let version = StateDBVersion::from_block_hash(block_ver);
    StateDBTransaction::from_version(store_txn, version)
}

#[test]
fn insert_and_get() {
    let store = Store::open_tmp().unwrap();
    let state_db_txn = get_state_db_txn(&store, H256::zero());

    state_db_txn.insert_raw("0", &[0, 0], &[0, 0, 0]).unwrap();
    state_db_txn.insert_raw("1", &[1, 1], &[1, 1, 1]).unwrap();
    state_db_txn.insert_raw("1", &[2], &[2, 2, 2]).unwrap();
    state_db_txn.commit().unwrap();

    assert_eq!(vec![0u8, 0, 0].as_slice(), state_db_txn.get("0", &[0, 0]).unwrap().as_ref());
    assert!(state_db_txn.get("0", &[1, 1]).is_none());
    assert_eq!(vec![1u8, 1, 1].as_slice(), state_db_txn.get("1", &[1, 1]).unwrap().as_ref());
    assert_eq!(vec![2u8, 2, 2].as_slice(), state_db_txn.get("1", &[2]).unwrap().as_ref());

    let iter = state_db_txn.get_iter("1", IteratorMode::Start);
    let mut r = HashMap::new();
    for (key, val) in iter {
        r.insert(key.to_vec(), val.to_vec());
    }
    assert_eq!(2, r.len());
    assert_eq!(Some(&vec![1u8, 1, 1]), r.get(&vec![1, 1]));
    assert_eq!(Some(&vec![2u8, 2, 2]), r.get(&vec![2]));
}

#[test]
fn delete() {
    let store = Store::open_tmp().unwrap();
    let state_db_txn = get_state_db_txn(&store, H256::zero());

    state_db_txn.insert_raw("1", &[2], &[1, 1, 1]).unwrap();
    state_db_txn.delete("1", &[2]).unwrap();

    assert!(state_db_txn.get("1", &[2]).is_none());
}
