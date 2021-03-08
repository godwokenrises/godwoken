use gw_store::{Store, traits::KVStore};
use gw_db::{IteratorMode};

use std::collections::HashMap;

#[test]
fn insert_and_get() {
    let store = Store::open_tmp().unwrap();
    let store_txn = store.begin_transaction();

    store_txn.insert_raw("0", &[0, 0], &[0, 0, 0]).unwrap();
    store_txn.insert_raw("1", &[1, 1], &[1, 1, 1]).unwrap();
    store_txn.insert_raw("1", &[2], &[2, 2, 2]).unwrap();
    store_txn.commit().unwrap();

    let store_txn = store.begin_transaction(); // new transaction can't be affected by uncommit transaction

    assert_eq!(vec![0u8, 0, 0].as_slice(), store_txn.get("0", &[0, 0]).unwrap().as_ref());
    assert!(store_txn.get("0", &[1, 1]).is_none());
    assert_eq!(vec![1u8, 1, 1].as_slice(), store_txn.get("1", &[1, 1]).unwrap().as_ref());
    assert_eq!(vec![2u8, 2, 2].as_slice(), store_txn.get("1", &[2]).unwrap().as_ref());

    let iter = store_txn.get_iter("1", IteratorMode::Start);
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
    let store_txn = store.begin_transaction();

    store_txn.insert_raw("1", &[2], &[1, 1, 1]).unwrap();
    store_txn.delete("1", &[2]).unwrap();

    let store_txn = store.begin_transaction(); // new transaction can't be affected by uncommit transaction
    assert!(store_txn.get("1", &[2]).is_none());
}

#[test]
fn insert_without_commit() {
    let store = Store::open_tmp().unwrap();
    let store_txn = store.begin_transaction();

    store_txn.insert_raw("0", &[0, 0], &[0, 0, 0]).unwrap();
    store_txn.insert_raw("1", &[1, 1], &[1, 1, 1]).unwrap();
    // without commit

    let store_txn = store.begin_transaction(); // new transaction can't be affected by uncommit transaction

    assert!(store_txn.get("0", &[0, 0]).is_none());
    assert!(store_txn.get("1", &[1, 1]).is_none());
}

#[test]
fn intersect_transactions() {
    let store = Store::open_tmp().unwrap();
    let state_txn_1 = store.begin_transaction();
    let state_txn_2 = store.begin_transaction();
    let state_txn_3 = store.begin_transaction();
    let state_txn_4 = store.begin_transaction();

    state_txn_2.insert_raw("1", &[1, 1], &[1, 1, 1]).unwrap();
    // state_txn_2 insert but without commit

    assert!(state_txn_1.get("1", &[1, 1]).is_none());
    assert_eq!(vec![1, 1, 1].as_slice(), state_txn_2.get("1", &[1, 1]).unwrap().as_ref());
    assert!(state_txn_3.get("1", &[1, 1]).is_none());
    assert!(state_txn_4.get("1", &[1, 1]).is_none());

    state_txn_4.insert_raw("1", &[2, 2], &[2, 2, 2]).unwrap();
    state_txn_4.commit().unwrap();

    assert_eq!(vec![2, 2, 2].as_slice(), state_txn_1.get("1", &[2, 2]).unwrap().as_ref());
    assert_eq!(vec![2, 2, 2].as_slice(), state_txn_2.get("1", &[2, 2]).unwrap().as_ref());
    assert_eq!(vec![2, 2, 2].as_slice(), state_txn_3.get("1", &[2, 2]).unwrap().as_ref());
    assert_eq!(vec![2, 2, 2].as_slice(), state_txn_4.get("1", &[2, 2]).unwrap().as_ref());

    // overwrite state_txn_2's insert without commit
    state_txn_4.insert_raw("1", &[1, 1], &[0, 0, 0]).unwrap();
    state_txn_4.commit().unwrap();

    assert_eq!(vec![0, 0, 0].as_slice(), state_txn_1.get("1", &[1, 1]).unwrap().as_ref());
    assert_eq!(vec![1, 1, 1].as_slice(), state_txn_2.get("1", &[1, 1]).unwrap().as_ref()); // no change
    assert_eq!(vec![0, 0, 0].as_slice(), state_txn_3.get("1", &[1, 1]).unwrap().as_ref());
    assert_eq!(vec![0, 0, 0].as_slice(), state_txn_4.get("1", &[1, 1]).unwrap().as_ref());
}
