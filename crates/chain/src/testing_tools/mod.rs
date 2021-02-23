use crate::{
    chain::{Chain, L1Action, L1ActionContext, ProduceBlockResult, SyncEvent, SyncParam},
    mem_pool::MemPool,
    next_block_context::NextBlockContext,
};
use gw_common::blake2b::new_blake2b;
use gw_config::{ChainConfig, GenesisConfig};
use gw_generator::{
    account_lock_manage::{always_success::AlwaysSuccess, AccountLockManage},
    backend_manage::BackendManage,
    genesis::init_genesis,
    Generator,
};
use gw_store::Store;
use gw_types::{
    bytes::Bytes,
    packed::{
        CellOutput, DepositionRequest, HeaderInfo, RawTransaction, RollupConfig, Script,
        Transaction, WitnessArgs,
    },
    prelude::*,
};
use lazy_static::lazy_static;
use parking_lot::Mutex;
use std::{fs, io::Read, path::PathBuf, sync::Arc};

const SCRIPT_DIR: &'static str = "../../build/debug";
const ALWAYS_SUCCESS_PATH: &'static str = "always-success";

lazy_static! {
    pub static ref ALWAYS_SUCCESS_PROGRAM: Bytes = {
        let mut buf = Vec::new();
        let mut path = PathBuf::new();
        path.push(&SCRIPT_DIR);
        path.push(&ALWAYS_SUCCESS_PATH);
        let mut f = fs::File::open(&path).expect("load program");
        f.read_to_end(&mut buf).expect("read program");
        Bytes::from(buf.to_vec())
    };
    pub static ref ALWAYS_SUCCESS_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&ALWAYS_SUCCESS_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
}

pub fn setup_chain(rollup_type_script: Script, rollup_config: RollupConfig) -> Chain {
    let mut account_lock_manage = AccountLockManage::default();
    account_lock_manage.register_lock_algorithm(
        ALWAYS_SUCCESS_CODE_HASH.clone().into(),
        Box::new(AlwaysSuccess),
    );
    setup_chain_with_account_lock_manage(rollup_type_script, rollup_config, account_lock_manage)
}

pub fn setup_chain_with_account_lock_manage(
    rollup_type_script: Script,
    rollup_config: RollupConfig,
    account_lock_manage: AccountLockManage,
) -> Chain {
    let store = Store::open_tmp().unwrap();
    let genesis_config = GenesisConfig { timestamp: 0 };
    let genesis_header_info = HeaderInfo::default();
    let backend_manage = BackendManage::default();
    let config = ChainConfig {
        rollup_type_script,
        rollup_config: rollup_config.clone(),
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
        &rollup_config,
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

pub fn build_sync_tx(
    rollup_cell: CellOutput,
    produce_block_result: ProduceBlockResult,
) -> Transaction {
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

pub fn apply_block_result(
    chain: &mut Chain,
    rollup_cell: CellOutput,
    nb_ctx: NextBlockContext,
    block_result: ProduceBlockResult,
    deposition_requests: Vec<DepositionRequest>,
) {
    let transaction = build_sync_tx(rollup_cell, block_result);
    let header_info = HeaderInfo::default();

    let update = L1Action {
        context: L1ActionContext::SubmitTxs {
            deposition_requests,
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
