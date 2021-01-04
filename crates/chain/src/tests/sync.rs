use crate::{
    chain::{Chain, L1Action, L1ActionContext, SyncEvent, SyncParam},
    next_block_context::NextBlockContext,
    tx_pool::TxPool,
};
use gw_config::{ChainConfig, GenesisConfig};
use gw_generator::{
    account_lock_manage::AccountLockManage, backend_manage::BackendManage, Generator,
};
use gw_store::Store;
use gw_types::{
    packed::{
        CellOutput, GlobalState, HeaderInfo, L2Block, RawL2Block, RawTransaction, Script,
        Transaction, WitnessArgs,
    },
    prelude::*,
};
use parking_lot::Mutex;
use std::sync::Arc;

#[test]
fn test_sync_a_block() {
    // setup
    let store = Store::open_tmp().unwrap();
    let genesis_config = GenesisConfig { timestamp: 0 };
    let genesis_header_info = HeaderInfo::default();
    let backend_manage = BackendManage::default();
    let account_lock_manage = AccountLockManage::default();
    let rollup_type_script = Script::default();
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
        .init_genesis(&genesis_config, genesis_header_info)
        .unwrap();
    let tip = store.get_tip_block().unwrap().unwrap();
    let tx_pool = TxPool::create(
        store.new_overlay().unwrap(),
        Arc::clone(&generator),
        &tip,
        nb_ctx,
    )
    .unwrap();
    let mut chain = Chain::create(config, store, generator, Arc::new(Mutex::new(tx_pool))).unwrap();
    let timestamp = timestamp + 1000;
    let nb_ctx = NextBlockContext {
        aggregator_id,
        timestamp,
    };

    // build update tx
    let global_state = GlobalState::default();
    let rollup_cell = CellOutput::new_builder()
        .type_(Some(rollup_type_script).pack())
        .build();
    let transaction = {
        let block_number = 1;
        let raw_l2block = RawL2Block::new_builder()
            .number(block_number.pack())
            .build();
        let l2block = L2Block::new_builder().raw(raw_l2block).build();
        let witness = WitnessArgs::new_builder()
            .output_type(Pack::<_>::pack(&Some(l2block.as_bytes())))
            .build();
        let raw = RawTransaction::new_builder()
            .outputs(vec![rollup_cell].pack())
            .outputs_data(vec![global_state.as_bytes()].pack())
            .build();
        Transaction::new_builder()
            .raw(raw)
            .witnesses(vec![witness.as_bytes()].pack())
            .build()
    };
    let header_info = HeaderInfo::default();

    let update = L1Action {
        context: L1ActionContext::SubmitTxs {
            deposition_requests: Default::default(),
        },
        transaction,
        header_info,
    };
    let param = SyncParam {
        updates: vec![update],
        reverts: Default::default(),
        next_block_context: nb_ctx,
    };
    let event = chain.sync(param).unwrap();
    assert_eq!(event, SyncEvent::Success);
}
