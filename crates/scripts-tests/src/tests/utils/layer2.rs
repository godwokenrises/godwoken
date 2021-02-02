use gw_chain::{
    chain::{Chain, ProduceBlockParam},
    mem_pool::{MemPool, PackageParam},
    next_block_context::NextBlockContext,
};
use gw_config::{ChainConfig, GenesisConfig};
use gw_generator::{
    account_lock_manage::{always_success::AlwaysSuccess, AccountLockManage},
    backend_manage::BackendManage,
    genesis::init_genesis,
    Generator,
};
use gw_store::Store;
use gw_types::{
    core::ScriptHashType,
    packed::{HeaderInfo, Script},
    prelude::*,
};
use parking_lot::Mutex;
use std::sync::Arc;
pub const ALWAYS_SUCCESS_ACCOUNT_LOCK_CODE_HASH: [u8; 32] = [42u8; 32];

pub fn setup_chain(rollup_type_script: &gw_types::packed::Script) -> Chain {
    let store = Store::open_tmp().unwrap();
    let genesis_config = GenesisConfig { timestamp: 0 };
    let genesis_header_info = HeaderInfo::default();
    let backend_manage = BackendManage::default();
    let mut account_lock_manage = AccountLockManage::default();
    account_lock_manage.register_lock_algorithm(
        ALWAYS_SUCCESS_ACCOUNT_LOCK_CODE_HASH.into(),
        Box::new(AlwaysSuccess),
    );
    let config = ChainConfig {
        rollup_type_script: rollup_type_script.clone(),
    };
    let rollup_script_hash = config.rollup_type_script.hash().into();
    let generator = Arc::new(Generator::new(
        backend_manage,
        account_lock_manage,
        rollup_script_hash,
    ));
    let block_producer_id = 0;
    let timestamp = 0;
    let nb_ctx = NextBlockContext {
        block_producer_id,
        timestamp,
    };
    init_genesis(
        &store,
        &genesis_config,
        genesis_header_info,
        rollup_script_hash,
    )
    .unwrap();
    let tip = store.get_tip_block().unwrap();
    let mem_pool = MemPool::create(
        store.new_overlay().unwrap(),
        Arc::clone(&generator),
        &tip,
        nb_ctx,
    )
    .unwrap();
    Chain::create(config, store, generator, Arc::new(Mutex::new(mem_pool))).unwrap()
}
