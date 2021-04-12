use crate::testing_tool::chain::{build_sync_tx, construct_block, setup_chain};
use gw_chain::chain::{L1Action, L1ActionContext, RevertedL1Action, SyncEvent, SyncParam};
use gw_common::{state::State, H256};
use gw_store::state_db::{StateDBTransaction, StateDBVersion};
use gw_types::{
    packed::{CellOutput, DepositionRequest, GlobalState, L2BlockCommittedInfo, Script},
    prelude::*,
};

#[test]
fn test_sync_a_block() {
    let rollup_type_script = Script::default();
    let mut chain = setup_chain(rollup_type_script.clone(), Default::default());

    let user_script = Script::new_builder().args(vec![42].pack()).build();
    let deposition = DepositionRequest::new_builder()
        .capacity(100u64.pack())
        .script(user_script)
        .build();
    let block_result = {
        let mem_pool = chain.mem_pool().lock();
        construct_block(&chain, &mem_pool, vec![deposition.clone()]).unwrap()
    };
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
    let l2block_committed_info = L2BlockCommittedInfo::default();

    let update = L1Action {
        context: L1ActionContext::SubmitTxs {
            deposition_requests: vec![deposition.clone()],
        },
        transaction,
        l2block_committed_info,
    };
    let param = SyncParam {
        updates: vec![update],
        reverts: Default::default(),
    };
    let event = chain.sync(param).unwrap();
    assert_eq!(event, SyncEvent::Success);
    drop(chain);
}

#[test]
fn test_layer1_fork() {
    let rollup_type_script = Script::default();
    let mut chain = setup_chain(rollup_type_script.clone(), Default::default());
    let rollup_cell = CellOutput::new_builder()
        .type_(Some(rollup_type_script.clone()).pack())
        .build();

    // build fork block 1
    let fork_action = {
        // build fork from another chain to avoid mess up the tx pool
        let charlie_script = Script::new_builder().args(vec![7].pack()).build();
        let deposition = DepositionRequest::new_builder()
            .capacity(120u64.pack())
            .script(charlie_script)
            .build();
        let chain = setup_chain(rollup_type_script.clone(), Default::default());
        let mem_pool = chain.mem_pool().lock();
        let block_result = construct_block(&chain, &mem_pool, vec![deposition.clone()]).unwrap();

        L1Action {
            context: L1ActionContext::SubmitTxs {
                deposition_requests: vec![deposition],
            },
            transaction: build_sync_tx(rollup_cell.clone(), block_result),
            l2block_committed_info: L2BlockCommittedInfo::new_builder()
                .number(1u64.pack())
                .build(),
        }
    };
    // update block 1
    let alice_script = Script::new_builder().args(vec![42].pack()).build();
    let deposition = DepositionRequest::new_builder()
        .capacity(100u64.pack())
        .script(alice_script)
        .build();
    let block_result = {
        let mem_pool = chain.mem_pool().lock();
        construct_block(&chain, &mem_pool, vec![deposition.clone()]).unwrap()
    };
    let action1 = L1Action {
        context: L1ActionContext::SubmitTxs {
            deposition_requests: vec![deposition.clone()],
        },
        transaction: build_sync_tx(rollup_cell.clone(), block_result),
        l2block_committed_info: L2BlockCommittedInfo::new_builder()
            .number(1u64.pack())
            .build(),
    };
    let param = SyncParam {
        updates: vec![action1.clone()],
        reverts: Default::default(),
    };
    let event = chain.sync(param).unwrap();
    assert_eq!(event, SyncEvent::Success);
    // update block 2
    let bob_script = Script::new_builder().args(vec![43].pack()).build();
    let deposition = DepositionRequest::new_builder()
        .capacity(500u64.pack())
        .script(bob_script)
        .build();
    let block_result = {
        let mem_pool = chain.mem_pool().lock();
        construct_block(&chain, &mem_pool, vec![deposition.clone()]).unwrap()
    };
    let action2 = L1Action {
        context: L1ActionContext::SubmitTxs {
            deposition_requests: vec![deposition],
        },
        transaction: build_sync_tx(rollup_cell.clone(), block_result),
        l2block_committed_info: L2BlockCommittedInfo::new_builder()
            .number(2u64.pack())
            .build(),
    };
    let param = SyncParam {
        updates: vec![action2.clone()],
        reverts: Default::default(),
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
                l2block_committed_info,
                context,
            } = action;
            RevertedL1Action {
                prev_global_state,
                transaction,
                l2block_committed_info,
                context,
            }
        })
        .collect::<Vec<_>>();
    let forks = vec![fork_action];

    let param = SyncParam {
        updates: forks,
        reverts,
    };
    let event = chain.sync(param).unwrap();
    assert_eq!(event, SyncEvent::Success);
    let tip_block = chain.store().get_tip_block().unwrap();
    let tip_block_number: u64 = tip_block.raw().number().unpack();
    assert_eq!(tip_block_number, 1);

    // check account SMT, should be able to calculate account state root
    {
        let db = chain.store().begin_transaction();
        let db = StateDBTransaction::from_version(
            &db,
            StateDBVersion::from_block_hash(tip_block.hash().into()),
        )
        .unwrap();
        let tree = db.account_state_tree().unwrap();
        let current_account_root = tree.calculate_root().unwrap();
        let expected_account_root: H256 = tip_block.raw().post_account().merkle_root().unpack();
        assert_eq!(
            current_account_root, expected_account_root,
            "check account tree"
        );
    }
}
