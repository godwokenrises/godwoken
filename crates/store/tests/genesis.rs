use gw_common::{sparse_merkle_tree::H256, state::State};
use gw_config::GenesisConfig;
use gw_generator::traits::CodeStore;
use gw_store::{genesis::build_genesis, Store};
use gw_types::{core::ScriptHashType, packed::HeaderInfo, prelude::*};
use std::convert::TryInto;

const GENESIS_BLOCK_HASH: [u8; 32] = [
    152, 241, 22, 26, 170, 65, 84, 153, 169, 81, 127, 211, 235, 204, 123, 110, 87, 232, 161, 82,
    247, 244, 68, 76, 108, 126, 5, 115, 60, 35, 246, 180,
];

#[test]
fn test_init_genesis() {
    let config = GenesisConfig { timestamp: 42 };
    let genesis = build_genesis(&config).unwrap();
    let genesis_block_hash: [u8; 32] = genesis.genesis.hash();
    assert_eq!(genesis_block_hash, GENESIS_BLOCK_HASH);
    let header_info = HeaderInfo::default();
    let store: Store = Store::open_tmp().unwrap();
    store.init_genesis(&config, header_info).unwrap();
    let db = store.begin_transaction();
    // check init values
    assert_ne!(db.get_block_smt_root().unwrap(), H256::zero());
    assert_ne!(db.get_account_smt_root().unwrap(), H256::zero());
    let tree = db.account_state_tree().unwrap();
    assert!(tree.get_account_count().unwrap() > 0);
    // get reserved account's script
    let meta_contract_script_hash = tree.get_script_hash(0).expect("script hash");
    assert_ne!(meta_contract_script_hash, H256::zero());
    let script = tree.get_script(&meta_contract_script_hash).expect("script");
    let hash_type: ScriptHashType = script.hash_type().try_into().unwrap();
    assert!(hash_type == ScriptHashType::Data);
    let code_hash: [u8; 32] = script.code_hash().unpack();
    assert_ne!(code_hash, [0u8; 32]);
}
