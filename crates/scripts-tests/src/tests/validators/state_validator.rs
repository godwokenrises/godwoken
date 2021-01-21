use super::{STATE_VALIDATOR_CODE_HASH, STATE_VALIDATOR_PROGRAM};
use crate::tests::utils::layer1::{
    always_success_script, build_resolved_tx, build_simple_tx, random_out_point, DummyDataLoader,
    ALWAYS_SUCCESS_PROGRAM, MAX_CYCLES,
};
use ckb_script::TransactionScriptsVerifier;
use ckb_types::{
    packed::{CellDep, CellOutput},
    prelude::Pack as CKBPack,
};
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
    let aggregator_id = 0;
    let timestamp = 0;
    let nb_ctx = NextBlockContext {
        aggregator_id,
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

fn state_validator_script() -> ckb_types::packed::Script {
    ckb_types::packed::Script::new_builder()
        .code_hash(CKBPack::pack(&*STATE_VALIDATOR_CODE_HASH))
        .hash_type(ScriptHashType::Data.into())
        .build()
}

#[test]
fn test_state_validator() {
    let rollup_type_script = {
        Script::new_builder()
            .code_hash(Pack::pack(&*STATE_VALIDATOR_CODE_HASH))
            .hash_type(ScriptHashType::Data.into())
            .build()
    };
    // setup chain
    let chain = setup_chain(&rollup_type_script);
    // deploy scripts
    let mut data_loader = DummyDataLoader::default();
    let always_success_dep = {
        let always_success_out_point = random_out_point();
        data_loader.cells.insert(
            always_success_out_point.clone(),
            (
                CellOutput::new_builder()
                    .capacity(CKBPack::pack(&(ALWAYS_SUCCESS_PROGRAM.len() as u64)))
                    .build(),
                ALWAYS_SUCCESS_PROGRAM.clone(),
            ),
        );
        CellDep::new_builder()
            .out_point(always_success_out_point)
            .build()
    };
    let state_validator_dep = {
        let state_validator_out_point = random_out_point();
        data_loader.cells.insert(
            state_validator_out_point.clone(),
            (
                CellOutput::new_builder()
                    .capacity(CKBPack::pack(&(STATE_VALIDATOR_PROGRAM.len() as u64)))
                    .build(),
                STATE_VALIDATOR_PROGRAM.clone(),
            ),
        );
        CellDep::new_builder()
            .out_point(state_validator_out_point)
            .build()
    };
    // create a rollup cell
    let capacity = 1000_00000000u64;
    let spend_cell = CellOutput::new_builder()
        .lock(always_success_script())
        .capacity(CKBPack::pack(&capacity))
        .build();
    let global_state = chain.local_state.last_global_state();
    let rollup_cell = CellOutput::new_builder()
        .lock(always_success_script())
        .type_(CKBPack::pack(&Some(state_validator_script())))
        .capacity(CKBPack::pack(&capacity))
        .build();
    let old_rollup_cell_data = global_state.as_bytes();
    let tx = build_simple_tx(
        &mut data_loader,
        (spend_cell, Default::default()),
        (rollup_cell.clone(), old_rollup_cell_data.clone()),
    )
    .as_advanced_builder()
    .cell_dep(always_success_dep.clone())
    .cell_dep(state_validator_dep.clone())
    .build();
    let resolved_tx = build_resolved_tx(&data_loader, &tx);
    let mut verifier = TransactionScriptsVerifier::new(&resolved_tx, &data_loader);
    verifier.set_debug_printer(|_script, msg| println!("[script debug] {}", msg));
    verifier.verify(MAX_CYCLES).expect("return success");
    // submit a new block
    let mem_pool_package = {
        let param = PackageParam {
            deposition_requests: Vec::new(),
            max_withdrawal_capacity: std::u128::MAX,
        };
        chain.mem_pool.lock().package(param).unwrap()
    };
    let param = ProduceBlockParam { aggregator_id: 0 };
    let block_result = chain.produce_block(param, mem_pool_package).unwrap();
    // verify submit block
    let rollup_cell_data = block_result.global_state.as_bytes();
    let witness = ckb_types::packed::WitnessArgs::new_builder()
        .output_type(CKBPack::pack(&Some(block_result.block.as_bytes())))
        .build();
    let tx = build_simple_tx(
        &mut data_loader,
        (rollup_cell.clone(), old_rollup_cell_data),
        (rollup_cell, rollup_cell_data),
    )
    .as_advanced_builder()
    .cell_dep(always_success_dep.clone())
    .cell_dep(state_validator_dep.clone())
    .witness(CKBPack::pack(&witness.as_bytes()))
    .build();
    let resolved_tx = build_resolved_tx(&data_loader, &tx);
    let mut verifier = TransactionScriptsVerifier::new(&resolved_tx, &data_loader);
    verifier.set_debug_printer(|_script, msg| println!("[script debug] {}", msg));
    verifier.verify(MAX_CYCLES).expect("return success");
}
