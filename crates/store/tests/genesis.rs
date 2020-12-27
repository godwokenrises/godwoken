use gw_common::{
    sparse_merkle_tree::{default_store::DefaultStore, H256},
    state::State,
};
use gw_config::GenesisConfig;
use gw_store::{genesis::build_genesis, Store};
use gw_types::packed::HeaderInfo;

#[test]
fn test_init_genesis() {
    let config = GenesisConfig { timestamp: 42 };
    let genesis = build_genesis(&config).unwrap();
    let header_info = HeaderInfo::default();
    let mut store: Store<DefaultStore<H256>> = Store::default();
    store.init_genesis(genesis, header_info).unwrap();
    assert_ne!(store.account_smt().root(), &H256::zero());
    let meta_contract_script_hash = store.get_script_hash(0).expect("script hash");
    assert_ne!(meta_contract_script_hash, H256::zero());
}
