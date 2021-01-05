use crate::{
    chain::{Chain, L1Action, L1ActionContext, RevertedL1Action, SyncEvent, SyncParam},
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
        .init_genesis(&genesis_config, genesis_header_info)
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

fn build_sync_tx(block_number: u64, rollup_cell: CellOutput) -> Transaction {
    let global_state = GlobalState::default();
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

    // build update tx
    let rollup_cell = CellOutput::new_builder()
        .type_(Some(rollup_type_script).pack())
        .build();
    let transaction = build_sync_tx(1, rollup_cell);
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

#[test]
fn test_layer1_fork() {
    let rollup_type_script = Script::default();
    let mut chain = setup_chain(&rollup_type_script);

    let aggregator_id = 0;
    let timestamp = 1000;
    let nb_ctx = NextBlockContext {
        aggregator_id,
        timestamp,
    };

    // update 2 blocks
    let rollup_cell = CellOutput::new_builder()
        .type_(Some(rollup_type_script).pack())
        .build();
    let updates = vec![
        L1Action {
            context: L1ActionContext::SubmitTxs {
                deposition_requests: Default::default(),
            },
            transaction: build_sync_tx(1, rollup_cell.clone()),
            header_info: HeaderInfo::new_builder().number(1u64.pack()).build(),
        },
        L1Action {
            context: L1ActionContext::SubmitTxs {
                deposition_requests: Default::default(),
            },
            transaction: build_sync_tx(2, rollup_cell.clone()),
            header_info: HeaderInfo::new_builder().number(2u64.pack()).build(),
        },
    ];
    let param = SyncParam {
        updates: updates.clone(),
        reverts: Default::default(),
        next_block_context: nb_ctx.clone(),
    };
    let event = chain.sync(param).unwrap();
    assert_eq!(event, SyncEvent::Success);
    let tip_block = chain.store().get_tip_block().unwrap();
    let tip_block_number: u64 = tip_block.raw().number().unpack();
    assert_eq!(tip_block_number, 2);

    // revert blocks
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
    let forks = vec![L1Action {
        context: L1ActionContext::SubmitTxs {
            deposition_requests: Default::default(),
        },
        transaction: build_sync_tx(1, rollup_cell.clone()),
        header_info: HeaderInfo::new_builder().number(1u64.pack()).build(),
    }];

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
}
