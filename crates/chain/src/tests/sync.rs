use super::{build_sync_tx, setup_chain};
use crate::{
    chain::{L1Action, L1ActionContext, ProduceBlockParam, RevertedL1Action, SyncEvent, SyncParam},
    mem_pool::PackageParam,
    next_block_context::NextBlockContext,
};
use gw_common::{state::State, H256};
use gw_types::{
    packed::{CellOutput, DepositionRequest, GlobalState, HeaderInfo, Script},
    prelude::*,
};

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
    let package_param = PackageParam {
        deposition_requests: vec![deposition.clone()],
        max_withdrawal_capacity: std::u128::MAX,
    };
    let mem_pool_package = chain.mem_pool.lock().package(package_param).unwrap();
    let param = ProduceBlockParam { aggregator_id };
    let block_result = chain.produce_block(param, mem_pool_package).unwrap();
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
        let chain = setup_chain(&rollup_type_script);
        let package_param = PackageParam {
            deposition_requests: vec![deposition.clone()],
            max_withdrawal_capacity: std::u128::MAX,
        };
        let mem_pool_package = chain.mem_pool.lock().package(package_param).unwrap();
        let param = ProduceBlockParam { aggregator_id };
        let block_result = chain.produce_block(param, mem_pool_package).unwrap();

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
    let package_param = PackageParam {
        deposition_requests: vec![deposition.clone()],
        max_withdrawal_capacity: std::u128::MAX,
    };
    let mem_pool_package = chain.mem_pool.lock().package(package_param).unwrap();
    let param = ProduceBlockParam { aggregator_id };
    let block_result = chain.produce_block(param, mem_pool_package).unwrap();
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
    let package_param = PackageParam {
        deposition_requests: vec![deposition.clone()],
        max_withdrawal_capacity: std::u128::MAX,
    };
    let mem_pool_package = chain.mem_pool.lock().package(package_param).unwrap();
    let param = ProduceBlockParam { aggregator_id };
    let block_result = chain.produce_block(param, mem_pool_package).unwrap();
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
