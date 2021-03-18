use super::*;

use crate::store_impl::Store;
use gw_db::DBRawIterator;

use std::collections::HashMap;

#[test]
fn insert_and_get() {
    let store = Store::open_tmp().unwrap();
    let store_txn = store.begin_transaction();

    store_txn.insert_raw("0", &[0, 0], &[0, 0, 0]).unwrap();
    store_txn.insert_raw("1", &[1, 1], &[1, 1, 1]).unwrap();
    store_txn.insert_raw("1", &[2], &[2, 2, 2]).unwrap();
    store_txn.commit().unwrap();

    // new transaction can't be affected by uncommit transaction
    let store_txn = store.begin_transaction();

    assert_eq!(
        vec![0u8, 0, 0].into_boxed_slice(),
        store_txn.get("0", &[0, 0]).unwrap()
    );
    assert!(store_txn.get("0", &[1, 1]).is_none());
    assert_eq!(
        vec![1u8, 1, 1].into_boxed_slice(),
        store_txn.get("1", &[1, 1]).unwrap()
    );
    assert_eq!(
        vec![2u8, 2, 2].into_boxed_slice(),
        store_txn.get("1", &[2]).unwrap()
    );

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
    store_txn.commit().unwrap();

    // new transaction is not affected by uncommit transaction
    let store_txn = store.begin_transaction();
    assert!(store_txn.get("1", &[2]).is_none());
}

#[test]
fn insert_without_commit() {
    let store = Store::open_tmp().unwrap();
    let store_txn = store.begin_transaction();

    // insert without commit
    store_txn.insert_raw("0", &[0, 0], &[0, 0, 0]).unwrap();
    store_txn.insert_raw("1", &[1, 1], &[1, 1, 1]).unwrap();

    // new transaction can't be affected by uncommit transaction
    let store_txn = store.begin_transaction();

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

    // state_txn_2 insert key without commit
    state_txn_2.insert_raw("1", &[1, 1], &[1, 1, 1]).unwrap();

    assert!(state_txn_1.get("1", &[1, 1]).is_none());
    assert_eq!(
        vec![1, 1, 1].into_boxed_slice(),
        state_txn_2.get("1", &[1, 1]).unwrap()
    );
    assert!(state_txn_3.get("1", &[1, 1]).is_none());
    assert!(state_txn_4.get("1", &[1, 1]).is_none());

    state_txn_4.insert_raw("1", &[2, 2], &[2, 2, 2]).unwrap();
    state_txn_4.commit().unwrap();

    // Transaction isolation level: Read Committed
    assert_eq!(
        vec![2, 2, 2].into_boxed_slice(),
        state_txn_1.get("1", &[2, 2]).unwrap()
    );
    assert_eq!(
        vec![2, 2, 2].into_boxed_slice(),
        state_txn_2.get("1", &[2, 2]).unwrap()
    );
    assert_eq!(
        vec![2, 2, 2].into_boxed_slice(),
        state_txn_3.get("1", &[2, 2]).unwrap()
    );
    assert_eq!(
        vec![2, 2, 2].into_boxed_slice(),
        state_txn_4.get("1", &[2, 2]).unwrap()
    );

    // overwrite state_txn_2's key inserted without commit
    state_txn_4.insert_raw("1", &[1, 1], &[0, 0, 0]).unwrap();
    state_txn_4.commit().unwrap();

    assert_eq!(
        vec![0, 0, 0].into_boxed_slice(),
        state_txn_1.get("1", &[1, 1]).unwrap()
    );
    assert_eq!(
        vec![1, 1, 1].into_boxed_slice(),
        state_txn_2.get("1", &[1, 1]).unwrap() // keep modified
    );
    assert_eq!(
        vec![0, 0, 0].into_boxed_slice(),
        state_txn_3.get("1", &[1, 1]).unwrap()
    );
    assert_eq!(
        vec![0, 0, 0].into_boxed_slice(),
        state_txn_4.get("1", &[1, 1]).unwrap()
    );

    // RosksDB's PessimisticTransaction mode will lock the key when insert
    // RosksDB's OptimisticTransaction mode won't lock the key when insert
    // but check conflict when commit
    // gw_store::Store use OptimisticTransaction mode by default
    assert!(state_txn_2.commit().is_err());
}

#[test]
fn seek_for_prev() {
    let store = Store::open_tmp().unwrap();
    let store_txn = store.begin_transaction();

    store_txn.insert_raw("1", &[0], &[0, 0, 0]).unwrap();
    store_txn.insert_raw("1", &[1], &[1, 1, 1]).unwrap();
    store_txn.insert_raw("1", &[2], &[2, 2, 2]).unwrap();
    store_txn.commit().unwrap();

    let store_txn = store.begin_transaction();
    assert_eq!(
        vec![0u8, 0, 0].into_boxed_slice(),
        store_txn.get("1", &[0]).unwrap()
    );
    assert_eq!(
        vec![1u8, 1, 1].into_boxed_slice(),
        store_txn.get("1", &[1]).unwrap()
    );
    assert_eq!(
        vec![2u8, 2, 2].into_boxed_slice(),
        store_txn.get("1", &[2]).unwrap()
    );

    let iter = store_txn.get_iter("1", IteratorMode::Start);

    let mut r = HashMap::new();
    for (key, val) in iter {
        r.insert(key.to_vec(), val.to_vec());
    }

    assert_eq!(3, r.len());
    assert_eq!(Some(&vec![0u8, 0, 0]), r.get(&vec![0]));
    assert_eq!(Some(&vec![1u8, 1, 1]), r.get(&vec![1]));
    assert_eq!(Some(&vec![2u8, 2, 2]), r.get(&vec![2]));

    let iter = store_txn.get_iter("1", IteratorMode::Start);
    let mut raw_iter: DBRawIterator = iter.into();
    raw_iter.seek_for_prev([5]);
    assert_eq!(&[2], raw_iter.key().unwrap());
    assert_eq!(&[2, 2, 2], raw_iter.value().unwrap());

    raw_iter.seek_for_prev([2]);
    assert_eq!(&[2], raw_iter.key().unwrap());
    assert_eq!(&[2, 2, 2], raw_iter.value().unwrap());

    raw_iter.seek_for_prev([1]);
    assert_eq!(&[1], raw_iter.key().unwrap());
    assert_eq!(&[1, 1, 1], raw_iter.value().unwrap());
}

#[test]
fn seek_for_prev_with_suffix() {
    let store = Store::open_tmp().unwrap();
    let store_txn = store.begin_transaction();

    let (block_num_1, tx_idx_5) = (1u64, 5u32);
    let (block_num_2, tx_idx_7) = (2u64, 7u32);
    let (block_num_256, tx_idx_2) = (256u64, 2u32);

    let (key_1, value_1) = ([1u8], [1u8]);
    let (key_2, value_2) = ([2u8, 2], [2u8, 2]);
    let (key_3, value_3) = ([3u8, 3, 3], [3u8, 3, 3]);

    let key_1_with_ver_1_5 = [
        &key_1[..],
        &block_num_1.to_be_bytes(),
        &tx_idx_5.to_be_bytes(),
    ]
    .concat();
    assert_eq!(
        vec![1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 5],
        key_1_with_ver_1_5
    );

    let key_2_with_ver_2_7 = [
        &key_2[..],
        &block_num_2.to_be_bytes(),
        &tx_idx_7.to_be_bytes(),
    ]
    .concat();
    assert_eq!(
        vec![2, 2, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 7],
        key_2_with_ver_2_7
    );

    let key_3_with_ver_256_2 = [
        &key_3[..],
        &block_num_256.to_be_bytes(),
        &tx_idx_2.to_be_bytes(),
    ]
    .concat();
    assert_eq!(
        vec![3, 3, 3, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 2],
        key_3_with_ver_256_2
    );

    store_txn
        .insert_raw("1", &key_1_with_ver_1_5, &value_1)
        .unwrap();
    store_txn
        .insert_raw("1", &key_2_with_ver_2_7, &value_2)
        .unwrap();
    store_txn
        .insert_raw("1", &key_3_with_ver_256_2, &value_3)
        .unwrap();

    // construct keys not in db
    let key_3_with_ver_257_1 = [&key_3[..], &257u64.to_be_bytes(), &9u32.to_be_bytes()].concat();
    let key_3_with_ver_256_9 = [
        &key_3[..],
        &block_num_256.to_be_bytes(),
        &9u32.to_be_bytes(),
    ]
    .concat();
    let key_3_with_ver_256_1 = [
        &key_3[..],
        &block_num_256.to_be_bytes(),
        &1u32.to_be_bytes(),
    ]
    .concat();
    let key_2_with_ver_2_6 = [&key_2[..], &block_num_2.to_be_bytes(), &6u32.to_be_bytes()].concat();
    let key_1_with_ver_1_4 = [&key_1[..], &block_num_1.to_be_bytes(), &4u32.to_be_bytes()].concat();

    let iter = store_txn.get_iter("1", IteratorMode::Start);
    let mut raw_iter: DBRawIterator = iter.into();

    raw_iter.seek_for_prev(key_3_with_ver_257_1);
    assert_eq!(&key_3_with_ver_256_2, raw_iter.key().unwrap());
    assert_eq!(&value_3, raw_iter.value().unwrap());

    raw_iter.seek_for_prev(key_3_with_ver_256_9);
    assert_eq!(&key_3_with_ver_256_2, raw_iter.key().unwrap());
    assert_eq!(&value_3, raw_iter.value().unwrap());

    raw_iter.seek_for_prev(key_3_with_ver_256_2.clone());
    assert_eq!(&key_3_with_ver_256_2, raw_iter.key().unwrap());
    assert_eq!(&value_3, raw_iter.value().unwrap());

    let n = key_3_with_ver_256_1.len();
    raw_iter.seek_for_prev(key_3_with_ver_256_1);
    assert_eq!(&key_2_with_ver_2_7, raw_iter.key().unwrap());
    assert_eq!(
        key_2,
        raw_iter.key().unwrap()[..key_2_with_ver_2_7.len() - 12]
    );
    assert_eq!(&value_2, raw_iter.value().unwrap());
    assert_ne!(key_3, raw_iter.key().unwrap()[..n - 12]);

    let n = key_2_with_ver_2_6.len();
    raw_iter.seek_for_prev(key_2_with_ver_2_6);
    assert_eq!(&key_1_with_ver_1_5, raw_iter.key().unwrap());
    assert_eq!(&value_1, raw_iter.value().unwrap());
    assert_ne!(key_1, raw_iter.key().unwrap()[..n - 12]);

    raw_iter.seek_for_prev(key_1_with_ver_1_4);
    assert_eq!(false, raw_iter.valid());
    assert!(raw_iter.key().is_none());
}
