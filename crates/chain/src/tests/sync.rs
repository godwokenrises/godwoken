use crate::{
    chain::{
        Chain, L1Action, L1ActionContext, ProduceBlockParam, ProduceBlockResult, RevertedL1Action,
        SyncEvent, SyncParam,
    },
    next_block_context::NextBlockContext,
    tx_pool::TxPool,
};
use gw_common::{state::State, H256};
use gw_config::{ChainConfig, GenesisConfig};
use gw_generator::{
    account_lock_manage::AccountLockManage, backend_manage::BackendManage, Generator,
};
use gw_store::Store;
use gw_types::{
    packed::{
        CellOutput, DepositionRequest, GlobalState, HeaderInfo, RawTransaction, Script,
        Transaction, WitnessArgs,
    },
    prelude::*,
};
use parking_lot::Mutex;
use std::sync::Arc;

fn setup_chain(rollup_type_script: &Script) -> Chain {
    let store = Store::open_tmp().unwrap();
    let genesis_config = GenesisConfig { timestamp: 0 };
    let genesis_header_info = HeaderInfo::default();
    let backend_manage = BackendManage::default();
    let account_lock_manage = AccountLockManage::default();
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
    let tx_pool = TxPool::create(
        store.new_overlay().unwrap(),
        Arc::clone(&generator),
        &tip,
        nb_ctx,
    )
    .unwrap();
    Chain::create(config, store, generator, Arc::new(Mutex::new(tx_pool))).unwrap()
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

#[test]
fn test_sync_a_block() {
    let rollup_type_script = Script::default();
    let mut chain = setup_chain(&rollup_type_script);

    let aggregator_id = 0;
    let timestamp = 1000;
    let nb_ctx = NextBlockContext {
        aggregator_id,
        timestamp,
    };

    let user_script = Script::new_builder().args(vec![42].pack()).build();
    let deposition = DepositionRequest::new_builder()
        .capacity(100u64.pack())
        .script(user_script)
        .build();
    let param = ProduceBlockParam {
        aggregator_id,
        deposition_requests: vec![deposition.clone()],
    };
    let block_result = chain.produce_block(param).unwrap();
    assert_eq!(
        {
            let tip_block_number: u64 = chain
                .store()
                .get_tip_block()
                .unwrap()
                .raw()
                .number()
                .unpack();
            tip_block_number
        },
        0
    );
    let rollup_cell = CellOutput::new_builder()
        .type_(Some(rollup_type_script).pack())
        .build();
    let transaction = build_sync_tx(rollup_cell, block_result);
    let header_info = HeaderInfo::default();

    let update = L1Action {
        context: L1ActionContext::SubmitTxs {
            deposition_requests: vec![deposition.clone()],
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

#[test]
fn test_layer1_fork() {
    let rollup_type_script = Script::default();
    let mut chain = setup_chain(&rollup_type_script);
    let rollup_cell = CellOutput::new_builder()
        .type_(Some(rollup_type_script.clone()).pack())
        .build();

    let aggregator_id = 0;
    let timestamp = 1000;
    let nb_ctx = NextBlockContext {
        aggregator_id,
        timestamp,
    };

    // build fork block 1
    let fork_action = {
        // build fork from another chain to avoid mess up the tx pool
        let charlie_script = Script::new_builder().args(vec![7].pack()).build();
        let deposition = DepositionRequest::new_builder()
            .capacity(120u64.pack())
            .script(charlie_script)
            .build();
        let param = ProduceBlockParam {
            aggregator_id,
            deposition_requests: vec![deposition.clone()],
        };
        let chain = setup_chain(&rollup_type_script);
        let block_result = chain.produce_block(param).unwrap();

        L1Action {
            context: L1ActionContext::SubmitTxs {
                deposition_requests: vec![deposition],
            },
            transaction: build_sync_tx(rollup_cell.clone(), block_result),
            header_info: HeaderInfo::new_builder().number(1u64.pack()).build(),
        }
    };
    // update block 1
    let alice_script = Script::new_builder().args(vec![42].pack()).build();
    let deposition = DepositionRequest::new_builder()
        .capacity(100u64.pack())
        .script(alice_script)
        .build();
    let param = ProduceBlockParam {
        aggregator_id,
        deposition_requests: vec![deposition.clone()],
    };
    let block_result = chain.produce_block(param).unwrap();
    let action1 = L1Action {
        context: L1ActionContext::SubmitTxs {
            deposition_requests: vec![deposition.clone()],
        },
        transaction: build_sync_tx(rollup_cell.clone(), block_result),
        header_info: HeaderInfo::new_builder().number(1u64.pack()).build(),
    };
    let param = SyncParam {
        updates: vec![action1.clone()],
        reverts: Default::default(),
        next_block_context: nb_ctx.clone(),
    };
    let event = chain.sync(param).unwrap();
    assert_eq!(event, SyncEvent::Success);
    // update block 2
    let bob_script = Script::new_builder().args(vec![43].pack()).build();
    let deposition = DepositionRequest::new_builder()
        .capacity(500u64.pack())
        .script(bob_script)
        .build();
    let param = ProduceBlockParam {
        aggregator_id,
        deposition_requests: vec![deposition.clone()],
    };
    let block_result = chain.produce_block(param).unwrap();
    let action2 = L1Action {
        context: L1ActionContext::SubmitTxs {
            deposition_requests: vec![deposition],
        },
        transaction: build_sync_tx(rollup_cell.clone(), block_result),
        header_info: HeaderInfo::new_builder().number(2u64.pack()).build(),
    };
    let param = SyncParam {
        updates: vec![action2.clone()],
        reverts: Default::default(),
        next_block_context: nb_ctx.clone(),
    };
    let event = chain.sync(param).unwrap();
    assert_eq!(event, SyncEvent::Success);
    let tip_block = chain.store().get_tip_block().unwrap();
    let tip_block_number: u64 = tip_block.raw().number().unpack();
    assert_eq!(tip_block_number, 2);

    // revert blocks
    let updates = vec![action1, action2];
    let reverts = updates
        .into_iter()
        .rev()
        .map(|action| {
            let prev_global_state = GlobalState::default();
            let L1Action {
                transaction,
                header_info,
                context,
            } = action;
            RevertedL1Action {
                prev_global_state,
                transaction,
                header_info,
                context,
            }
        })
        .collect::<Vec<_>>();
    let forks = vec![fork_action];

    let param = SyncParam {
        updates: forks,
        reverts,
        next_block_context: nb_ctx,
    };
    let event = chain.sync(param).unwrap();
    assert_eq!(event, SyncEvent::Success);
    let tip_block = chain.store().get_tip_block().unwrap();
    let tip_block_number: u64 = tip_block.raw().number().unpack();
    assert_eq!(tip_block_number, 1);

    // check account SMT, should be able to calculate account state root
    {
        let db = chain.store().begin_transaction();
        let tree = db.account_state_tree().unwrap();
        let current_account_root = tree.calculate_root().unwrap();
        let expected_account_root: H256 = tip_block.raw().post_account().merkle_root().unpack();
        assert_eq!(
            current_account_root, expected_account_root,
            "check account tree"
        );
    }
}
