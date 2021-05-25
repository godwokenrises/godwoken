use crate::genesis::{build_genesis, init_genesis};
use gw_common::{sparse_merkle_tree::H256, state::State};
use gw_config::GenesisConfig;
use gw_store::{
    state_db::{CheckPoint, StateDBMode, StateDBTransaction},
    Store,
};
use gw_traits::CodeStore;
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{L2BlockCommittedInfo, RollupConfig},
    prelude::*,
};
use std::convert::TryInto;

const GENESIS_BLOCK_HASH: [u8; 32] = [
    161, 152, 90, 241, 75, 20, 189, 92, 201, 107, 247, 181, 136, 56, 95, 14, 128, 5, 91, 62, 125,
    47, 254, 126, 173, 225, 77, 162, 203, 173, 123, 57,
];

#[test]
fn test_init_genesis() {
    let meta_contract_code_hash = [1u8; 32];
    let rollup_script_hash: [u8; 32] = [42u8; 32];
    let config = GenesisConfig {
        timestamp: 42,
        meta_contract_validator_type_hash: meta_contract_code_hash.into(),
        rollup_config: RollupConfig::default().into(),
        rollup_type_hash: rollup_script_hash.into(),
        secp_data_dep: Default::default(),
    };
    let genesis = build_genesis(&config, Bytes::default()).unwrap();
    let genesis_block_hash: [u8; 32] = genesis.genesis.hash();
    assert_eq!(genesis_block_hash, GENESIS_BLOCK_HASH);
    let genesis_committed_info = L2BlockCommittedInfo::default();
    let store: Store = Store::open_tmp().unwrap();
    init_genesis(&store, &config, genesis_committed_info, Bytes::default()).unwrap();
    let db = store.begin_transaction();
    // check init values
    assert_ne!(db.get_block_smt_root().unwrap(), H256::zero());
    assert_ne!(db.get_account_smt_root().unwrap(), H256::zero());
    let state_db =
        StateDBTransaction::from_checkpoint(&db, CheckPoint::from_genesis(), StateDBMode::Genesis)
            .unwrap();
    let tree = state_db.account_state_tree().unwrap();
    assert!(tree.get_account_count().unwrap() > 0);
    // get reserved account's script
    let meta_contract_script_hash = tree.get_script_hash(0).expect("script hash");
    assert_ne!(meta_contract_script_hash, H256::zero());
    let script = tree.get_script(&meta_contract_script_hash).expect("script");
    let args: Bytes = script.args().unpack();
    assert_eq!(&args, &rollup_script_hash[..]);
    let hash_type: ScriptHashType = script.hash_type().try_into().unwrap();
    assert!(hash_type == ScriptHashType::Type);
    let code_hash: [u8; 32] = script.code_hash().unpack();
    assert_eq!(code_hash, meta_contract_code_hash);
}
