use crate::{
    state_db::{StateDBTransaction, StateDBVersion},
    traits::KVStore,
    Store,
};
use gw_common::H256;
use gw_db::schema::{Col, COLUMN_ACCOUNT_SMT_BRANCH, COLUMN_ACCOUNT_SMT_LEAF};

fn insert_to_state_db(
    db: &Store,
    col: Col,
    bloch_hash: H256,
    block_number: u64,
    tx_index: u32,
    key: &[u8],
    value: &[u8],
) {
    let store_txn = db.begin_transaction();
    let state_db_txn = StateDBTransaction::from_tx_index(
        &store_txn,
        StateDBVersion::from_block_hash(bloch_hash),
        block_number,
        tx_index,
    );
    state_db_txn.insert_raw(col, key, value).unwrap();
    state_db_txn.commit().unwrap();
}

fn insert_branch_to_state_db(
    db: &Store,
    bloch_hash: H256,
    block_number: u64,
    tx_index: u32,
    key: &[u8],
    value: &[u8],
) {
    insert_to_state_db(
        db,
        COLUMN_ACCOUNT_SMT_BRANCH,
        bloch_hash,
        block_number,
        tx_index,
        key,
        value,
    );
}

fn insert_leaf_to_state_db(
    db: &Store,
    bloch_hash: H256,
    block_number: u64,
    tx_index: u32,
    key: &[u8],
    value: &[u8],
) {
    insert_to_state_db(
        db,
        COLUMN_ACCOUNT_SMT_LEAF,
        bloch_hash,
        block_number,
        tx_index,
        key,
        value,
    );
}

fn get_from_state_db(
    db: &Store,
    col: Col,
    bloch_hash: H256,
    block_number: u64,
    tx_index: u32,
    key: &[u8],
) -> Option<Box<[u8]>> {
    let store_txn = db.begin_transaction();
    let state_db = StateDBTransaction::from_tx_index(
        &store_txn,
        StateDBVersion::from_block_hash(bloch_hash),
        block_number,
        tx_index,
    );
    state_db.get(col, key)
}

fn get_from_branch(
    db: &Store,
    bloch_hash: H256,
    block_number: u64,
    tx_index: u32,
    key: &[u8],
) -> Option<Box<[u8]>> {
    get_from_state_db(
        db,
        COLUMN_ACCOUNT_SMT_BRANCH,
        bloch_hash,
        block_number,
        tx_index,
        key,
    )
}

fn get_from_leaf(
    db: &Store,
    bloch_hash: H256,
    block_number: u64,
    tx_index: u32,
    key: &[u8],
) -> Option<Box<[u8]>> {
    get_from_state_db(
        db,
        COLUMN_ACCOUNT_SMT_LEAF,
        bloch_hash,
        block_number,
        tx_index,
        key,
    )
}

#[test]
fn detach_block() {
    let db = Store::open_tmp().unwrap();

    // block 1
    let (block_1_hash, block_1_number) = (H256::from([1; 32]), 1u64);
    insert_branch_to_state_db(&db, block_1_hash, block_1_number, 1u32, &[1], &[1, 1, 1]);
    insert_branch_to_state_db(&db, block_1_hash, block_1_number, 2u32, &[1], &[2, 2, 2]);
    insert_branch_to_state_db(&db, block_1_hash, block_1_number, 3u32, &[2], &[3, 3, 3]);
    insert_branch_to_state_db(&db, block_1_hash, block_1_number, 4u32, &[2], &[4, 4, 4]);

    insert_leaf_to_state_db(&db, block_1_hash, block_1_number, 1u32, &[1, 1], &[11]);
    insert_leaf_to_state_db(&db, block_1_hash, block_1_number, 2u32, &[1, 1], &[22]);
    insert_leaf_to_state_db(&db, block_1_hash, block_1_number, 3u32, &[2, 2], &[33]);
    insert_leaf_to_state_db(&db, block_1_hash, block_1_number, 4u32, &[2, 2], &[44]);

    insert_to_state_db(&db, "1", block_1_hash, block_1_number, 1u32, &[1], &[1]);
    insert_to_state_db(&db, "2", block_1_hash, block_1_number, 1u32, &[2], &[2]);

    // block 2
    let (block_2_hash, block_2_number) = (H256::from([2; 32]), 2u64);
    insert_branch_to_state_db(&db, block_2_hash, block_2_number, 1u32, &[1], &[1, 1, 1]);
    insert_branch_to_state_db(&db, block_2_hash, block_2_number, 2u32, &[1], &[2, 2, 2]);
    insert_branch_to_state_db(&db, block_2_hash, block_2_number, 3u32, &[2], &[3, 3, 3]);
    insert_branch_to_state_db(&db, block_2_hash, block_2_number, 4u32, &[2], &[4, 4, 4]);

    insert_leaf_to_state_db(&db, block_2_hash, block_2_number, 1u32, &[1, 1], &[1]);
    insert_leaf_to_state_db(&db, block_2_hash, block_2_number, 2u32, &[1, 1], &[2]);
    insert_leaf_to_state_db(&db, block_2_hash, block_2_number, 3u32, &[2, 2], &[3]);
    insert_leaf_to_state_db(&db, block_2_hash, block_2_number, 4u32, &[2, 2], &[4]);

    insert_to_state_db(&db, "1", block_2_hash, block_2_number, 5u32, &[5], &[5]);
    insert_to_state_db(&db, "2", block_2_hash, block_2_number, 6u32, &[6], &[6]);

    // attach new block with the same block number when detaching block 1
    let (block_new_hash, block_new_number) = (H256::from([3; 32]), block_1_number);
    insert_branch_to_state_db(&db, block_new_hash, block_new_number, 1u32, &[5], &[5, 5]);
    insert_branch_to_state_db(&db, block_new_hash, block_new_number, 2u32, &[5], &[6, 6]);
    insert_branch_to_state_db(&db, block_new_hash, block_new_number, 3u32, &[6], &[7, 7]);
    insert_branch_to_state_db(&db, block_new_hash, block_new_number, 4u32, &[6], &[8, 8]);

    insert_leaf_to_state_db(&db, block_new_hash, block_new_number, 1u32, &[5, 5], &[55]);
    insert_leaf_to_state_db(&db, block_new_hash, block_new_number, 2u32, &[5, 5], &[66]);
    insert_leaf_to_state_db(&db, block_new_hash, block_new_number, 3u32, &[6, 6], &[77]);
    insert_leaf_to_state_db(&db, block_new_hash, block_new_number, 4u32, &[6, 6], &[88]);

    insert_to_state_db(&db, "1", block_new_hash, block_new_number, 1u32, &[5], &[5]);
    insert_to_state_db(&db, "2", block_new_hash, block_new_number, 1u32, &[6], &[6]);

    // detach block 1,
    let store_txn = db.begin_transaction();
    store_txn
        .clear_account_state_tree_for_test(block_1_hash)
        .unwrap();
    store_txn.commit().unwrap();

    // check block 1
    assert!(get_from_branch(&db, block_1_hash, block_1_number, 1u32, &[1]).is_none());
    assert!(get_from_branch(&db, block_1_hash, block_1_number, 2u32, &[1]).is_none());
    assert!(get_from_branch(&db, block_1_hash, block_1_number, 3u32, &[2]).is_none());
    assert!(get_from_branch(&db, block_1_hash, block_1_number, 4u32, &[2]).is_none());
    assert!(get_from_leaf(&db, block_1_hash, block_1_number, 1u32, &[1, 1]).is_none());
    assert!(get_from_leaf(&db, block_1_hash, block_1_number, 2u32, &[1, 1]).is_none());
    assert!(get_from_leaf(&db, block_1_hash, block_1_number, 3u32, &[2, 2]).is_none());
    assert!(get_from_leaf(&db, block_1_hash, block_1_number, 4u32, &[2, 2]).is_none());
    assert_eq!(
        vec![1].into_boxed_slice(),
        get_from_state_db(&db, "1", block_1_hash, block_1_number, 1u32, &[1]).unwrap()
    );
    assert_eq!(
        vec![2].into_boxed_slice(),
        get_from_state_db(&db, "2", block_1_hash, block_1_number, 1u32, &[2]).unwrap()
    );

    // check block 2
    assert_eq!(
        vec![1, 1, 1].into_boxed_slice(),
        get_from_branch(&db, block_2_hash, block_2_number, 1u32, &[1]).unwrap()
    );
    assert_eq!(
        vec![2, 2, 2].into_boxed_slice(),
        get_from_branch(&db, block_2_hash, block_2_number, 2u32, &[1]).unwrap()
    );
    assert_eq!(
        vec![3, 3, 3].into_boxed_slice(),
        get_from_branch(&db, block_2_hash, block_2_number, 3u32, &[2]).unwrap()
    );
    assert_eq!(
        vec![4, 4, 4].into_boxed_slice(),
        get_from_branch(&db, block_2_hash, block_2_number, 4u32, &[2]).unwrap()
    );
    assert_eq!(
        vec![1].into_boxed_slice(),
        get_from_leaf(&db, block_2_hash, block_2_number, 1u32, &[1, 1]).unwrap()
    );
    assert_eq!(
        vec![2].into_boxed_slice(),
        get_from_leaf(&db, block_2_hash, block_2_number, 2u32, &[1, 1]).unwrap()
    );
    assert_eq!(
        vec![3].into_boxed_slice(),
        get_from_leaf(&db, block_2_hash, block_2_number, 3u32, &[2, 2]).unwrap()
    );
    assert_eq!(
        vec![4].into_boxed_slice(),
        get_from_leaf(&db, block_2_hash, block_2_number, 4u32, &[2, 2]).unwrap()
    );
    assert_eq!(
        vec![5].into_boxed_slice(),
        get_from_state_db(&db, "1", block_2_hash, block_2_number, 1u32, &[5]).unwrap()
    );
    assert_eq!(
        vec![6].into_boxed_slice(),
        get_from_state_db(&db, "2", block_2_hash, block_2_number, 1u32, &[6]).unwrap()
    );

    // check block new
    assert_eq!(
        vec![5, 5].into_boxed_slice(),
        get_from_branch(&db, block_new_hash, block_new_number, 1u32, &[5]).unwrap()
    );
    assert_eq!(
        vec![6, 6].into_boxed_slice(),
        get_from_branch(&db, block_new_hash, block_new_number, 2u32, &[5]).unwrap()
    );
    assert_eq!(
        vec![7, 7].into_boxed_slice(),
        get_from_branch(&db, block_new_hash, block_new_number, 3u32, &[6]).unwrap()
    );
    assert_eq!(
        vec![8, 8].into_boxed_slice(),
        get_from_branch(&db, block_new_hash, block_new_number, 4u32, &[6]).unwrap()
    );
    assert_eq!(
        vec![55].into_boxed_slice(),
        get_from_leaf(&db, block_new_hash, block_new_number, 1u32, &[5, 5]).unwrap()
    );
    assert_eq!(
        vec![66].into_boxed_slice(),
        get_from_leaf(&db, block_new_hash, block_new_number, 2u32, &[5, 5]).unwrap()
    );
    assert_eq!(
        vec![77].into_boxed_slice(),
        get_from_leaf(&db, block_new_hash, block_new_number, 3u32, &[6, 6]).unwrap()
    );
    assert_eq!(
        vec![88].into_boxed_slice(),
        get_from_leaf(&db, block_new_hash, block_new_number, 4u32, &[6, 6]).unwrap()
    );
    assert_eq!(
        vec![5].into_boxed_slice(),
        get_from_state_db(&db, "1", block_new_hash, block_new_number, 1u32, &[5]).unwrap()
    );
    assert_eq!(
        vec![6].into_boxed_slice(),
        get_from_state_db(&db, "2", block_new_hash, block_new_number, 1u32, &[6]).unwrap()
    );
}

#[test]
fn clear_last_account_state_record() {
    let db = Store::open_tmp().unwrap();

    let (block_2_hash, block_2_number) = (H256::from([2; 32]), 2u64);
    insert_branch_to_state_db(&db, block_2_hash, block_2_number, 1u32, &[1], &[1, 1, 1]);
    insert_branch_to_state_db(&db, block_2_hash, block_2_number, 2u32, &[1], &[2, 2, 2]);
    insert_branch_to_state_db(&db, block_2_hash, block_2_number, 3u32, &[2], &[3, 3, 3]);
    insert_branch_to_state_db(&db, block_2_hash, block_2_number, 4u32, &[2], &[4, 4, 4]);

    insert_leaf_to_state_db(&db, block_2_hash, block_2_number, 1u32, &[1, 1], &[1]);
    insert_leaf_to_state_db(&db, block_2_hash, block_2_number, 2u32, &[1, 1], &[2]);
    insert_leaf_to_state_db(&db, block_2_hash, block_2_number, 3u32, &[2, 2], &[3]);
    insert_leaf_to_state_db(&db, block_2_hash, block_2_number, 4u32, &[2, 2], &[4]);

    let store_txn = db.begin_transaction();
    store_txn
        .clear_account_state_record_for_test(block_2_hash)
        .unwrap();
    store_txn.commit().unwrap();

    let store_txn = db.begin_transaction();
    store_txn
        .clear_account_state_tree_for_test(block_2_hash)
        .unwrap();
    store_txn.commit().unwrap();

    assert_eq!(
        vec![1, 1, 1].into_boxed_slice(),
        get_from_branch(&db, block_2_hash, block_2_number, 1u32, &[1]).unwrap()
    );
    assert_eq!(
        vec![2, 2, 2].into_boxed_slice(),
        get_from_branch(&db, block_2_hash, block_2_number, 2u32, &[1]).unwrap()
    );
    assert_eq!(
        vec![3, 3, 3].into_boxed_slice(),
        get_from_branch(&db, block_2_hash, block_2_number, 3u32, &[2]).unwrap()
    );
    assert_eq!(
        vec![4, 4, 4].into_boxed_slice(),
        get_from_branch(&db, block_2_hash, block_2_number, 4u32, &[2]).unwrap()
    );
    assert_eq!(
        vec![1].into_boxed_slice(),
        get_from_leaf(&db, block_2_hash, block_2_number, 1u32, &[1, 1]).unwrap()
    );
    assert_eq!(
        vec![2].into_boxed_slice(),
        get_from_leaf(&db, block_2_hash, block_2_number, 2u32, &[1, 1]).unwrap()
    );
    assert_eq!(
        vec![3].into_boxed_slice(),
        get_from_leaf(&db, block_2_hash, block_2_number, 3u32, &[2, 2]).unwrap()
    );
    assert_eq!(
        vec![4].into_boxed_slice(),
        get_from_leaf(&db, block_2_hash, block_2_number, 4u32, &[2, 2]).unwrap()
    );
}
