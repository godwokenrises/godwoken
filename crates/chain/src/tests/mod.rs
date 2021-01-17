mod deposition_withdrawal;
mod sync;

use crate::{
    chain::{Chain, ProduceBlockResult},
    next_block_context::NextBlockContext,
    mem_pool::MemPool,
};
use gw_config::{ChainConfig, GenesisConfig};
use gw_generator::{
    account_lock_manage::{always_success::AlwaysSuccess, AccountLockManage},
    backend_manage::BackendManage,
    Generator,
};
use gw_store::Store;
use gw_types::{
    packed::{CellOutput, HeaderInfo, RawTransaction, Script, Transaction, WitnessArgs},
    prelude::*,
};
use parking_lot::Mutex;
use std::sync::Arc;

pub const ALWAYS_SUCCESS_ACCOUNT_LOCK_CODE_HASH: [u8; 32] = [42u8; 32];

pub fn setup_chain(rollup_type_script: &Script) -> Chain {
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
    let aggregator_id = 0;
    let timestamp = 0;
    let nb_ctx = NextBlockContext {
        aggregator_id,
        timestamp,
    };
    store
        .init_genesis(&genesis_config, genesis_header_info, rollup_script_hash)
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

fn build_sync_tx(rollup_cell: CellOutput, produce_block_result: ProduceBlockResult) -> Transaction {
    let ProduceBlockResult {
        block,
        global_state,
    } = produce_block_result;
    let witness = WitnessArgs::new_builder()
        .output_type(Pack::<_>::pack(&Some(block.as_bytes())))
        .build();
    let raw = RawTransaction::new_builder()
        .outputs(vec![rollup_cell].pack())
        .outputs_data(vec![global_state.as_bytes()].pack())
        .build();
    Transaction::new_builder()
        .raw(raw)
        .witnesses(vec![witness.as_bytes()].pack())
        .build()
}
