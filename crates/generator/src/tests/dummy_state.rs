use ckb_vm::Bytes;
use gw_common::{
    blake2b::new_blake2b, h256_ext::H256Ext, registry_address::RegistryAddress, smt::SMT,
    state::State, H256,
};
use gw_store::{
    smt::smt_store::SMTStateStore,
    snapshot::StoreSnapshot,
    state::{
        overlay::{mem_state::MemStateTree, mem_store::MemStore},
        MemStateDB,
    },
    Store,
};
use gw_traits::CodeStore;
use gw_types::{
    packed::Script,
    prelude::{Builder, Entity, Pack},
    U256,
};

use crate::{traits::StateExt, Error};

fn new_state(store: StoreSnapshot) -> MemStateDB {
    let smt = SMT::new(H256::zero(), SMTStateStore::new(MemStore::new(store)));
    let inner = MemStateTree::new(smt, 0);
    MemStateDB::new(inner)
}

#[test]
fn test_account_with_duplicate_script() {
    let db = Store::open_tmp().unwrap();
    let mut tree = new_state(db.get_snapshot());
    let script = Script::new_builder()
        .args([0u8; 42].pack())
        .hash_type(gw_types::core::ScriptHashType::Type.into())
        .build();

    // create duplicate account
    let id = tree.create_account_from_script(script.clone()).unwrap();
    assert_eq!(id, 0);
    let err = tree.create_account_from_script(script.clone()).unwrap_err();
    assert_eq!(
        err,
        Error::State(gw_common::error::Error::DuplicatedScriptHash)
    );

    // create duplicate account
    let err2 = tree.create_account(script.hash().into()).unwrap_err();
    assert_eq!(err2, gw_common::error::Error::DuplicatedScriptHash);
}

#[test]
fn test_query_account() {
    let db = Store::open_tmp().unwrap();
    let mut tree = new_state(db.get_snapshot());
    let script_a = Script::new_builder()
        .args([0u8; 42].pack())
        .hash_type(gw_types::core::ScriptHashType::Type.into())
        .build();
    let script_b = Script::new_builder()
        .args([1u8; 42].pack())
        .hash_type(gw_types::core::ScriptHashType::Type.into())
        .build();

    // query account info
    for (expected_id, script) in [script_a, script_b].iter().enumerate() {
        let id = tree.create_account_from_script(script.to_owned()).unwrap();
        assert_eq!(id, expected_id as u32);
        assert_eq!(tree.get_account_count().unwrap(), (expected_id + 1) as u32);
        assert_eq!(
            tree.get_account_id_by_script_hash(&script.hash().into())
                .unwrap()
                .unwrap(),
            id
        );
        assert_eq!(tree.get_script_hash(id).unwrap(), script.hash().into());
        assert_eq!(&tree.get_script(&script.hash().into()).unwrap(), script);
    }
}

#[test]
fn test_nonce() {
    let db = Store::open_tmp().unwrap();
    let mut tree = new_state(db.get_snapshot());
    let script = Script::new_builder()
        .args([0u8; 42].pack())
        .hash_type(gw_types::core::ScriptHashType::Type.into())
        .build();
    let id = tree.create_account_from_script(script).unwrap();
    assert_eq!(id, 0);
    // query account info
    for i in 1..15 {
        tree.set_nonce(id, i).unwrap();
        assert_eq!(tree.get_nonce(id).unwrap(), i);
    }
}

#[test]
fn test_kv() {
    let db = Store::open_tmp().unwrap();
    let mut tree = new_state(db.get_snapshot());
    let script = Script::new_builder()
        .args([0u8; 42].pack())
        .hash_type(gw_types::core::ScriptHashType::Type.into())
        .build();
    let id = tree.create_account_from_script(script).unwrap();
    assert_eq!(id, 0);
    // query account info
    for i in 1..15 {
        let key = H256::from_u32(i as u32);
        let value = H256::from_u32(i as u32);
        tree.update_value(id, key.as_slice(), value).unwrap();
        assert_eq!(tree.get_value(id, key.as_slice()).unwrap(), value);
    }
}

#[test]
fn test_sudt() {
    let db = Store::open_tmp().unwrap();
    let mut tree = new_state(db.get_snapshot());
    let script = Script::new_builder()
        .args([0u8; 42].pack())
        .hash_type(gw_types::core::ScriptHashType::Type.into())
        .build();
    let id = tree.create_account_from_script(script).unwrap();
    assert_eq!(id, 0);
    let script = Script::new_builder()
        .args([1u8; 42].pack())
        .hash_type(gw_types::core::ScriptHashType::Type.into())
        .build();
    let sudt_id = tree.create_account_from_script(script).unwrap();
    assert_eq!(sudt_id, 1);
    // mint sudt
    let user_a = RegistryAddress::new(0, vec![1u8; 20]);
    let user_b = RegistryAddress::new(0, vec![2u8; 20]);
    tree.mint_sudt(sudt_id, &user_a, U256::from(100u64))
        .unwrap();
    assert_eq!(
        tree.get_sudt_total_supply(sudt_id).unwrap(),
        U256::from(100)
    );
    tree.mint_sudt(sudt_id, &user_a, U256::from(230u64))
        .unwrap();
    assert_eq!(
        tree.get_sudt_total_supply(sudt_id).unwrap(),
        U256::from(330)
    );
    tree.mint_sudt(sudt_id, &user_b, U256::from(155u64))
        .unwrap();
    assert_eq!(
        tree.get_sudt_total_supply(sudt_id).unwrap(),
        U256::from(485)
    );
    // burn sudt
    tree.burn_sudt(sudt_id, &user_a, U256::from(85u64)).unwrap();
    assert_eq!(
        tree.get_sudt_total_supply(sudt_id).unwrap(),
        U256::from(400)
    );
    // overdraft
    let err = tree
        .burn_sudt(sudt_id, &user_b, U256::from(200u64))
        .unwrap_err();
    assert_eq!(err, gw_common::error::Error::AmountOverflow);
    assert_eq!(
        tree.get_sudt_total_supply(sudt_id).unwrap(),
        U256::from(400)
    );
    tree.burn_sudt(sudt_id, &user_b, U256::from(100u64))
        .unwrap();
    assert_eq!(
        tree.get_sudt_total_supply(sudt_id).unwrap(),
        U256::from(300)
    );
}

#[test]
fn test_data_hash() {
    let db = Store::open_tmp().unwrap();
    let mut tree = new_state(db.get_snapshot());
    let data = [42u8; 42];
    let data_hash = {
        let mut hasher = new_blake2b();
        let mut buf = [0u8; 32];
        hasher.update(&data);
        hasher.finalize(&mut buf);
        buf.into()
    };
    tree.insert_data(data_hash, data.to_vec().into());
    // query data
    assert_eq!(
        tree.get_data(&data_hash).unwrap(),
        Bytes::from(data.to_vec())
    );
    // store data hash
    assert!(!tree.is_data_hash_exist(&data_hash).unwrap());
    assert!(!tree.is_data_hash_exist(&H256::zero()).unwrap());
    tree.store_data_hash(data_hash).unwrap();
    assert!(tree.is_data_hash_exist(&data_hash).unwrap());
}
