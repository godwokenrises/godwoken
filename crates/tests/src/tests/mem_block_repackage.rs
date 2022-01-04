#![allow(clippy::mutable_key_type)]

use crate::testing_tool::{
    chain::{setup_chain, ALWAYS_SUCCESS_CODE_HASH},
    mem_pool_provider::DummyMemPoolProvider,
};

use gw_block_producer::{
    produce_block::{produce_block, ProduceBlockParam},
    replay_block::ReplayBlock,
};
use gw_common::H256;
use gw_generator::traits::StateExt;
use gw_mem_pool::pool::OutputParam;
use gw_runtime::block_on;
use gw_store::{mem_pool_state::MemStore, traits::chain_store::ChainStore};
use gw_types::{
    core::ScriptHashType,
    offchain::{CellInfo, CollectedCustodianCells, DepositInfo, RollupContext},
    packed::{CellOutput, DepositLockArgs, DepositRequest, OutPoint, Script},
    prelude::*,
};

use std::time::Duration;

#[test]
fn test_repackage_mem_block() {
    const DEPOSIT_CAPACITY: u64 = 1000_00000000;
    const DEPOSIT_AMOUNT: u128 = 0;

    let rollup_type_script = Script::default();
    let rollup_script_hash = rollup_type_script.hash().into();
    let chain = setup_chain(rollup_type_script);

    let users = (0..10).map(|_| random_always_success_script(&rollup_script_hash));
    let deposits = users.map(|user_script| {
        DepositRequest::new_builder()
            .capacity(DEPOSIT_CAPACITY.pack())
            .sudt_script_hash(H256::zero().pack())
            .amount(DEPOSIT_AMOUNT.pack())
            .script(user_script)
            .build()
    });

    let rollup_context = chain.generator().rollup_context();
    let deposit_cells: Vec<_> = deposits
        .map(|r| into_deposit_info_cell(rollup_context, r))
        .collect();

    let mem_pool = chain.mem_pool().as_ref().unwrap();
    let mut mem_pool = block_on(mem_pool.lock());
    let provider = DummyMemPoolProvider {
        deposit_cells,
        fake_blocktime: Duration::from_millis(0),
        collected_custodians: CollectedCustodianCells::default(),
    };
    mem_pool.set_provider(Box::new(provider));
    block_on(mem_pool.reset_mem_block()).unwrap();

    {
        let snap = chain.store().get_snapshot();
        let mem_store = MemStore::new(snap);
        let state = mem_store.state().unwrap();
        let tip_block = chain.store().get_tip_block().unwrap();

        assert_eq!(
            state.merkle_state().unwrap().as_slice(),
            tip_block.raw().post_account().as_slice()
        );
    }

    let (_, block_param) =
        block_on(mem_pool.output_mem_block(&OutputParam { retry_count: 1 })).unwrap();

    let deposit_cells = block_param.deposits.clone();

    // produce block
    let reverted_block_root: H256 = {
        let db = chain.store().begin_transaction();
        let smt = db.reverted_block_smt().unwrap();
        smt.root().to_owned()
    };
    let param = ProduceBlockParam {
        stake_cell_owner_lock_hash: random_always_success_script(&rollup_script_hash)
            .hash()
            .into(),
        reverted_block_root,
        rollup_config_hash: rollup_context.rollup_config.hash().into(),
        block_param,
    };
    let store = chain.store();
    let db = store.begin_transaction();
    let block_result = produce_block(&db, chain.generator(), param).unwrap();

    let deposit_requests: Vec<_> = deposit_cells.iter().map(|i| i.request.clone()).collect();
    ReplayBlock::replay(
        store,
        chain.generator(),
        &block_result.block,
        deposit_requests.as_slice(),
    )
    .unwrap()
}

fn random_always_success_script(rollup_script_hash: &H256) -> Script {
    let random_bytes: [u8; 32] = rand::random();
    Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.clone().pack())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.as_slice().to_vec();
            args.extend_from_slice(&random_bytes);
            args.pack()
        })
        .build()
}

fn into_deposit_info_cell(rollup_context: &RollupContext, request: DepositRequest) -> DepositInfo {
    let rollup_script_hash = rollup_context.rollup_script_hash;
    let deposit_lock_type_hash = rollup_context.rollup_config.deposit_script_type_hash();

    let lock_args = {
        let cancel_timeout = 0xc0000000000004b0u64;
        let mut buf: Vec<u8> = Vec::new();
        let deposit_args = DepositLockArgs::new_builder()
            .cancel_timeout(cancel_timeout.pack())
            .build();
        buf.extend(rollup_script_hash.as_slice());
        buf.extend(deposit_args.as_slice());
        buf
    };

    let out_point = OutPoint::new_builder()
        .tx_hash(rand::random::<[u8; 32]>().pack())
        .build();
    let lock_script = Script::new_builder()
        .code_hash(deposit_lock_type_hash)
        .hash_type(ScriptHashType::Type.into())
        .args(lock_args.pack())
        .build();
    let output = CellOutput::new_builder()
        .lock(lock_script)
        .capacity(request.capacity())
        .build();

    let cell = CellInfo {
        out_point,
        output,
        data: request.amount().as_bytes(),
    };

    DepositInfo { cell, request }
}
