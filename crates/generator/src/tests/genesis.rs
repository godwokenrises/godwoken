use crate::genesis::{build_genesis, init_genesis};
use gw_common::{sparse_merkle_tree::H256, state::State};
use gw_config::GenesisConfig;
use gw_store::{state::state_db::StateContext, Store};
use gw_traits::CodeStore;
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{L2BlockCommittedInfo, RollupConfig},
    prelude::*,
};
use std::convert::TryInto;

const GENESIS_BLOCK_HASH: [u8; 32] = [
    187, 142, 142, 7, 31, 178, 127, 168, 17, 104, 90, 201, 147, 60, 172, 114, 64, 236, 189, 139,
    249, 70, 207, 71, 71, 201, 149, 45, 36, 144, 66, 65,
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
    let tree = db.state_tree(StateContext::ReadOnly).unwrap();
    assert!(tree.get_account_count().unwrap() > 0);

    // check prev txs state
    let prev_txs_state: [u8; 32] = tree.calculate_state_checkpoint().unwrap().into();
    let genesis_prev_state_checkpoint: [u8; 32] = {
        let txs = genesis.genesis.as_reader().raw().submit_transactions();
        txs.prev_state_checkpoint().unpack()
    };
    assert_eq!(prev_txs_state, genesis_prev_state_checkpoint);

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
