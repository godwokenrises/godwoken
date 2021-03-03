use crate::genesis::{build_genesis, init_genesis};
use crate::types::RollupContext;
use gw_common::{sparse_merkle_tree::H256, state::State};
use gw_config::GenesisConfig;
use gw_store::{
    state_db::{StateDBTransaction, StateDBVersion},
    Store,
};
use gw_traits::CodeStore;
use gw_types::{
    core::ScriptHashType,
    packed::{HeaderInfo, RollupConfig},
    prelude::*,
};
use std::convert::TryInto;

const GENESIS_BLOCK_HASH: [u8; 32] = [
    246, 197, 150, 109, 12, 89, 50, 74, 28, 54, 195, 12, 149, 226, 102, 107, 161, 38, 3, 21, 134,
    159, 123, 96, 243, 134, 50, 196, 194, 112, 114, 127,
];

#[test]
fn test_init_genesis() {
    let config = GenesisConfig { timestamp: 42 };
    let rollup_context = RollupContext {
        rollup_config: RollupConfig::default(),
        rollup_script_hash: [42u8; 32].into(),
    };
    let genesis = build_genesis(&config, &rollup_context).unwrap();
    let genesis_block_hash: [u8; 32] = genesis.genesis.hash();
    assert_eq!(genesis_block_hash, GENESIS_BLOCK_HASH);
    let header_info = HeaderInfo::default();
    let store: Store = Store::open_tmp().unwrap();
    init_genesis(&store, &config, &rollup_context, header_info).unwrap();
    let db = store.begin_transaction();
    // check init values
    assert_ne!(db.get_block_smt_root().unwrap(), H256::zero());
    assert_ne!(db.get_account_smt_root().unwrap(), H256::zero());
    let state_db = StateDBTransaction::from_version(db, StateDBVersion::from_genesis());
    let tree = state_db.account_state_tree().unwrap();
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
