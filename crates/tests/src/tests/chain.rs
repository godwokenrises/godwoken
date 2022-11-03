use crate::testing_tool::{
    bad_block::generate_bad_block_using_first_withdrawal,
    chain::{
        build_sync_tx, construct_block, into_deposit_info_cell, restart_chain, setup_chain,
        ALWAYS_SUCCESS_CODE_HASH, DEFAULT_FINALITY_BLOCKS,
    },
};

use gw_block_producer::produce_block::ProduceBlockResult;
use gw_chain::chain::{
    Chain, ChallengeCell, L1Action, L1ActionContext, RevertL1ActionContext, RevertedL1Action,
    SyncEvent, SyncParam,
};
use gw_common::{builtins::CKB_SUDT_ACCOUNT_ID, ckb_decimal::CKBCapacity, state::State, H256};
use gw_store::{
    state::{history::history_state::RWConfig, BlockStateDB},
    traits::chain_store::ChainStore,
};
use gw_types::{
    bytes::Bytes,
    core::{ScriptHashType, Status},
    packed::{
        CellInput, CellOutput, DepositInfoVec, DepositRequest, GlobalState, RawWithdrawalRequest,
        Script, WithdrawalRequest, WithdrawalRequestExtra,
    },
    prelude::*,
};

const CKB: u64 = 100000000;

async fn produce_a_block(
    chain: &mut Chain,
    deposit: DepositRequest,
    rollup_cell: CellOutput,
    expected_tip: u64,
) -> SyncParam {
    let deposit_info_vec = DepositInfoVec::new_builder()
        .push(into_deposit_info_cell(chain.generator().rollup_context(), deposit).pack())
        .build();
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(chain, &mut mem_pool, deposit_info_vec.clone())
            .await
            .unwrap()
    };
    let l2block = block_result.block.clone();
    let transaction = build_sync_tx(rollup_cell, block_result);

    let update = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block,
            deposit_info_vec,
            deposit_asset_scripts: Default::default(),
            withdrawals: Default::default(),
        },
        transaction,
    };
    let param = SyncParam {
        updates: vec![update],
        reverts: Default::default(),
    };
    chain.sync(param.clone()).await.unwrap();
    assert!(chain.last_sync_event().is_success());
    chain.notify_new_tip().await.unwrap();

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
        expected_tip
    );
    param
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_produce_blocks() {
    let rollup_type_script = Script::default();
    let rollup_script_hash = rollup_type_script.hash();
    let mut chain = setup_chain(rollup_type_script.clone()).await;

    let rollup_cell = CellOutput::new_builder()
        .type_(Some(rollup_type_script).pack())
        .build();

    // block #1
    let user_script_a = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.clone().pack())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.to_vec();
            args.extend(&[42u8; 20]);
            args.pack()
        })
        .build();
    let deposit = DepositRequest::new_builder()
        .capacity((290u64 * CKB).pack())
        .script(user_script_a.clone())
        .registry_id(gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID.pack())
        .build();
    produce_a_block(&mut chain, deposit, rollup_cell.clone(), 1).await;

    // block #2
    let deposit = DepositRequest::new_builder()
        .capacity((400u64 * CKB).pack())
        .script(user_script_a.clone())
        .registry_id(gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID.pack())
        .build();
    produce_a_block(&mut chain, deposit, rollup_cell.clone(), 2).await;

    // block #3
    let user_script_b = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.clone().pack())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.to_vec();
            args.extend(&[50u8; 20]);
            args.pack()
        })
        .build();
    let deposit = DepositRequest::new_builder()
        .capacity((500u64 * CKB).pack())
        .script(user_script_b.clone())
        .registry_id(gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID.pack())
        .build();
    produce_a_block(&mut chain, deposit, rollup_cell, 3).await;

    // check state
    {
        let db = chain.store().begin_transaction();
        let tree = BlockStateDB::from_store(&db, RWConfig::readonly()).unwrap();
        let script_hash_a: H256 = user_script_a.hash().into();
        let script_hash_b: H256 = user_script_b.hash().into();
        let id_a = tree
            .get_account_id_by_script_hash(&script_hash_a)
            .unwrap()
            .unwrap();
        let id_b = tree
            .get_account_id_by_script_hash(&script_hash_b)
            .unwrap()
            .unwrap();
        // 0 is meta contract, 1 is ckb sudt 2 is eth reg, so the user id start from 3
        assert_eq!(id_a, 3);
        assert_eq!(id_b, 4);
        let a_addr = tree
            .get_registry_address_by_script_hash(
                gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID,
                &script_hash_a,
            )
            .unwrap()
            .unwrap();
        let b_addr = tree
            .get_registry_address_by_script_hash(
                gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID,
                &script_hash_b,
            )
            .unwrap()
            .unwrap();
        let balance_a = tree.get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &a_addr).unwrap();
        let balance_b = tree.get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &b_addr).unwrap();
        assert_eq!(balance_a, CKBCapacity::from_layer1(690 * CKB).to_layer2());
        assert_eq!(balance_b, CKBCapacity::from_layer1(500 * CKB).to_layer2());
    }

    drop(chain);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_layer1_fork() {
    let rollup_type_script = Script::default();
    let rollup_script_hash = rollup_type_script.hash();
    let mut chain = setup_chain(rollup_type_script.clone()).await;
    let rollup_cell = CellOutput::new_builder()
        .type_(Some(rollup_type_script.clone()).pack())
        .build();

    // build fork block 1
    let fork_action = {
        // build fork from another chain to avoid mess up the tx pool
        let charlie_script = Script::new_builder()
            .code_hash(ALWAYS_SUCCESS_CODE_HASH.clone().pack())
            .hash_type(ScriptHashType::Type.into())
            .args({
                let mut args = rollup_script_hash.to_vec();
                args.extend(&[7u8; 20]);
                args.pack()
            })
            .build();
        let deposit = DepositRequest::new_builder()
            .capacity((290u64 * CKB).pack())
            .script(charlie_script)
            .registry_id(gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID.pack())
            .build();
        let chain = setup_chain(rollup_type_script).await;
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        let deposit_info_vec = DepositInfoVec::new_builder()
            .push(into_deposit_info_cell(chain.generator().rollup_context(), deposit).pack())
            .build();
        let block_result = construct_block(&chain, &mut mem_pool, deposit_info_vec.clone())
            .await
            .unwrap();

        L1Action {
            context: L1ActionContext::SubmitBlock {
                l2block: block_result.block.clone(),
                deposit_info_vec,
                deposit_asset_scripts: Default::default(),
                withdrawals: Default::default(),
            },
            transaction: build_sync_tx(rollup_cell.clone(), block_result),
        }
    };
    // update block 1
    let alice_script = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.clone().pack())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.to_vec();
            args.extend(&[42u8; 20]);
            args.pack()
        })
        .build();
    let deposit = DepositRequest::new_builder()
        .capacity((400u64 * CKB).pack())
        .script(alice_script)
        .registry_id(gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID.pack())
        .build();
    let deposit_info_vec = DepositInfoVec::new_builder()
        .push(into_deposit_info_cell(chain.generator().rollup_context(), deposit).pack())
        .build();
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(&chain, &mut mem_pool, deposit_info_vec.clone())
            .await
            .unwrap()
    };
    let action1 = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block: block_result.block.clone(),
            deposit_info_vec,
            deposit_asset_scripts: Default::default(),
            withdrawals: Default::default(),
        },
        transaction: build_sync_tx(rollup_cell.clone(), block_result),
    };
    let param = SyncParam {
        updates: vec![action1],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    chain.notify_new_tip().await.unwrap();
    assert!(chain.last_sync_event().is_success());
    // update block 2
    let bob_script = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.clone().pack())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.to_vec();
            args.extend([43u8; 20]);
            args.pack()
        })
        .build();
    let deposit = DepositRequest::new_builder()
        .capacity((500u64 * CKB).pack())
        .script(bob_script)
        .registry_id(gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID.pack())
        .build();
    let deposit_info_vec = DepositInfoVec::new_builder()
        .push(into_deposit_info_cell(chain.generator().rollup_context(), deposit).pack())
        .build();
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(&chain, &mut mem_pool, deposit_info_vec.clone())
            .await
            .unwrap()
    };
    let action2 = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block: block_result.block.clone(),
            deposit_info_vec,
            deposit_asset_scripts: Default::default(),
            withdrawals: Default::default(),
        },
        transaction: build_sync_tx(rollup_cell, block_result),
    };
    let param = SyncParam {
        updates: vec![action2],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    chain.notify_new_tip().await.unwrap();
    assert!(chain.last_sync_event().is_success());
    let tip_block = chain.store().get_tip_block().unwrap();
    let tip_block_number: u64 = tip_block.raw().number().unpack();
    assert_eq!(tip_block_number, 2);

    // revert blocks
    let db = &chain.store().begin_transaction();
    let tip_block_parent_hash: H256 = tip_block.raw().parent_block_hash().unpack();
    let revert_action2 = {
        let prev_global_state = db
            .get_block_post_global_state(&tip_block_parent_hash)
            .unwrap()
            .unwrap();
        let context = RevertL1ActionContext::SubmitValidBlock { l2block: tip_block };
        RevertedL1Action {
            prev_global_state,
            context,
        }
    };
    let tip_parent_block = db.get_block(&tip_block_parent_hash).unwrap().unwrap();
    let tip_grandpa_block_hash: H256 = tip_parent_block.raw().parent_block_hash().unpack();
    let revert_action1 = {
        let prev_global_state = db
            .get_block_post_global_state(&tip_grandpa_block_hash)
            .unwrap()
            .unwrap();
        let context = RevertL1ActionContext::SubmitValidBlock {
            l2block: tip_parent_block,
        };
        RevertedL1Action {
            prev_global_state,
            context,
        }
    };
    let forks = vec![fork_action];

    let param = SyncParam {
        updates: forks,
        reverts: vec![revert_action2, revert_action1],
    };
    chain.sync(param).await.unwrap();
    assert!(chain.last_sync_event().is_success());

    let tip_block = chain.store().get_tip_block().unwrap();
    let tip_block_number: u64 = tip_block.raw().number().unpack();
    assert_eq!(tip_block_number, 1);

    // check account SMT, should be able to calculate account state root
    {
        let db = chain.store().begin_transaction();
        let mut tree = BlockStateDB::from_store(&db, RWConfig::readonly()).unwrap();
        let current_account_root = tree.finalise_root().unwrap();
        let expected_account_root: H256 = tip_block.raw().post_account().merkle_root().unpack();
        assert_eq!(
            current_account_root, expected_account_root,
            "check account tree"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_layer1_revert() {
    let rollup_type_script = Script::default();
    let rollup_script_hash = rollup_type_script.hash();
    let mut chain = setup_chain(rollup_type_script.clone()).await;
    let rollup_cell = CellOutput::new_builder()
        .type_(Some(rollup_type_script).pack())
        .build();

    let default_eoa_code_hash = chain
        .generator()
        .rollup_context()
        .rollup_config
        .allowed_eoa_type_hashes()
        .get(0)
        .expect("get default EoA hash")
        .hash();

    // update block 1
    let alice_script = Script::new_builder()
        .code_hash(default_eoa_code_hash.clone())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.to_vec();
            args.extend(&[42u8; 20]);
            args.pack()
        })
        .build();
    let deposit = DepositRequest::new_builder()
        .capacity((400u64 * CKB).pack())
        .script(alice_script.clone())
        .registry_id(gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID.pack())
        .build();
    let deposit_info_vec = DepositInfoVec::new_builder()
        .push(into_deposit_info_cell(chain.generator().rollup_context(), deposit).pack())
        .build();
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(&chain, &mut mem_pool, deposit_info_vec.clone())
            .await
            .unwrap()
    };
    let action1 = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block: block_result.block.clone(),
            deposit_info_vec,
            deposit_asset_scripts: Default::default(),
            withdrawals: Default::default(),
        },
        transaction: build_sync_tx(rollup_cell.clone(), block_result),
    };
    let param = SyncParam {
        updates: vec![action1],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    chain.notify_new_tip().await.unwrap();
    assert!(chain.last_sync_event().is_success());
    // update block 2
    let bob_script = Script::new_builder()
        .code_hash(default_eoa_code_hash)
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.to_vec();
            args.extend(&[43u8; 20]);
            args.pack()
        })
        .build();
    let deposit = DepositRequest::new_builder()
        .capacity((500u64 * CKB).pack())
        .script(bob_script.clone())
        .registry_id(gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID.pack())
        .build();
    let deposit_info_vec = DepositInfoVec::new_builder()
        .push(into_deposit_info_cell(chain.generator().rollup_context(), deposit).pack())
        .build();
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(&chain, &mut mem_pool, deposit_info_vec.clone())
            .await
            .unwrap()
    };
    let action2 = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block: block_result.block.clone(),
            deposit_info_vec,
            deposit_asset_scripts: Default::default(),
            withdrawals: Default::default(),
        },
        transaction: build_sync_tx(rollup_cell, block_result),
    };
    let param = SyncParam {
        updates: vec![action2.clone()],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    chain.notify_new_tip().await.unwrap();
    assert!(chain.last_sync_event().is_success());
    let tip_block = chain.store().get_tip_block().unwrap();
    let tip_block_number: u64 = tip_block.raw().number().unpack();
    assert_eq!(tip_block_number, 2);

    // revert blocks
    let updates = vec![action2.clone()];
    let reverts = updates
        .into_iter()
        .rev()
        .map(|action| {
            let prev_global_state = GlobalState::default();
            let L1Action {
                transaction: _,
                context,
            } = action;
            let l2block = match context {
                L1ActionContext::SubmitBlock { l2block, .. } => l2block,
                _ => unreachable!(),
            };
            let context = RevertL1ActionContext::SubmitValidBlock { l2block };
            RevertedL1Action {
                prev_global_state,
                context,
            }
        })
        .collect::<Vec<_>>();

    let param = SyncParam {
        updates: Default::default(),
        reverts,
    };
    chain.sync(param).await.unwrap();
    chain.notify_new_tip().await.unwrap();
    assert!(chain.last_sync_event().is_success());

    let tip_block = chain.store().get_tip_block().unwrap();
    let tip_block_number: u64 = tip_block.raw().number().unpack();
    assert_eq!(tip_block_number, 1);

    // check account SMT, should be able to calculate account state root
    {
        let db = chain.store().begin_transaction();
        let mut tree = BlockStateDB::from_store(&db, RWConfig::readonly()).unwrap();
        let current_account_root = tree.finalise_root().unwrap();
        let expected_account_root: H256 = tip_block.raw().post_account().merkle_root().unpack();
        assert_eq!(
            current_account_root, expected_account_root,
            "check account tree"
        );

        assert_eq!(tree.get_account_count().unwrap(), 4);
        let alice_script_hash: H256 = alice_script.hash().into();
        let alice_id = tree
            .get_account_id_by_script_hash(&alice_script_hash)
            .unwrap()
            .unwrap();
        assert_eq!(alice_id, 3);
        let alice_addr = tree
            .get_registry_address_by_script_hash(
                gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID,
                &alice_script_hash,
            )
            .unwrap()
            .unwrap();
        let alice_balance = tree
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &alice_addr)
            .unwrap();
        assert_eq!(
            alice_balance,
            CKBCapacity::from_layer1(400 * CKB).to_layer2()
        );

        let bob_id_opt = tree
            .get_account_id_by_script_hash(&bob_script.hash().into())
            .unwrap();
        assert!(bob_id_opt.is_none());
    }

    // execute block2 agnain
    let updates = vec![action2];
    let param = SyncParam {
        updates,
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    chain.notify_new_tip().await.unwrap();
    assert!(chain.last_sync_event().is_success());

    // check block2 agnain

    let tip_block = chain.store().get_tip_block().unwrap();
    let tip_block_number: u64 = tip_block.raw().number().unpack();
    assert_eq!(tip_block_number, 2);

    {
        let db = chain.store().begin_transaction();
        let mut tree = BlockStateDB::from_store(&db, RWConfig::readonly()).unwrap();
        let current_account_root = tree.finalise_root().unwrap();
        let expected_account_root: H256 = tip_block.raw().post_account().merkle_root().unpack();
        assert_eq!(
            current_account_root, expected_account_root,
            "check account tree"
        );

        assert_eq!(tree.get_account_count().unwrap(), 5);
        let alice_script_hash: H256 = alice_script.hash().into();
        let alice_id = tree
            .get_account_id_by_script_hash(&alice_script_hash)
            .unwrap()
            .unwrap();
        assert_eq!(alice_id, 3);
        let alice_addr = tree
            .get_registry_address_by_script_hash(
                gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID,
                &alice_script_hash,
            )
            .unwrap()
            .unwrap();
        let alice_balance = tree
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &alice_addr)
            .unwrap();
        assert_eq!(
            alice_balance,
            CKBCapacity::from_layer1(400 * CKB).to_layer2()
        );

        let bob_script_hash: H256 = bob_script.hash().into();
        let bob_id = tree
            .get_account_id_by_script_hash(&bob_script_hash)
            .unwrap()
            .unwrap();
        assert_eq!(bob_id, 4);
        let bob_addr = tree
            .get_registry_address_by_script_hash(
                gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID,
                &bob_script_hash,
            )
            .unwrap()
            .unwrap();

        let bob_balance = tree
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &bob_addr)
            .unwrap();
        assert_eq!(bob_balance, CKBCapacity::from_layer1(500 * CKB).to_layer2());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_sync_blocks() {
    let rollup_type_script = Script::default();
    let rollup_script_hash = rollup_type_script.hash();
    let mut chain1 = setup_chain(rollup_type_script.clone()).await;
    let mut chain2 = setup_chain(rollup_type_script.clone()).await;

    let rollup_cell = CellOutput::new_builder()
        .type_(Some(rollup_type_script).pack())
        .build();

    // block #1
    let user_script_a = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.clone().pack())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.to_vec();
            args.extend(&[42u8; 20]);
            args.pack()
        })
        .build();
    let sudt_script_hash: H256 = [42u8; 32].into();
    let deposit = DepositRequest::new_builder()
        .capacity((400u64 * CKB).pack())
        .script(user_script_a.clone())
        .sudt_script_hash(sudt_script_hash.pack())
        .registry_id(gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID.pack())
        .build();
    let sync_1 = produce_a_block(&mut chain1, deposit, rollup_cell.clone(), 1).await;

    // block #2
    let deposit = DepositRequest::new_builder()
        .capacity((400u64 * CKB).pack())
        .script(user_script_a.clone())
        .registry_id(gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID.pack())
        .build();
    let sync_2 = produce_a_block(&mut chain1, deposit, rollup_cell.clone(), 2).await;

    // block #3
    let user_script_b = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.clone().pack())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.to_vec();
            args.extend(&[50u8; 20]);
            args.pack()
        })
        .build();
    let deposit = DepositRequest::new_builder()
        .capacity((500u64 * CKB).pack())
        .script(user_script_b.clone())
        .sudt_script_hash(sudt_script_hash.pack())
        .registry_id(gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID.pack())
        .build();
    let sync_3 = produce_a_block(&mut chain1, deposit, rollup_cell, 3).await;

    drop(chain1);

    chain2.sync(sync_1).await.expect("success");
    assert!(chain2.last_sync_event().is_success());

    chain2.sync(sync_2).await.expect("success");
    assert!(chain2.last_sync_event().is_success());

    chain2.sync(sync_3).await.expect("success");
    assert!(chain2.last_sync_event().is_success());

    // check state
    {
        let db = &chain2.store().begin_transaction();
        let tip_block = db.get_tip_block().unwrap();
        let tip_block_number: u64 = tip_block.raw().number().unpack();
        let tip_block_hash = db.get_tip_block_hash().unwrap();
        assert_eq!(tip_block_hash, tip_block.hash().into());
        assert_eq!(tip_block_number, 3);

        let tree = BlockStateDB::from_store(db, RWConfig::readonly()).unwrap();
        let script_hash_a: H256 = user_script_a.hash().into();
        let id_a = tree
            .get_account_id_by_script_hash(&script_hash_a)
            .unwrap()
            .unwrap();
        let script_hash_b: H256 = user_script_b.hash().into();
        let id_b = tree
            .get_account_id_by_script_hash(&script_hash_b)
            .unwrap()
            .unwrap();
        // 0 is meta contract, 1 is ckb sudt, 2 is eth reg, so the user id start from 3
        assert_eq!(id_a, 3);
        assert_eq!(id_b, 5);
        let a_addr = tree
            .get_registry_address_by_script_hash(
                gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID,
                &script_hash_a,
            )
            .unwrap()
            .unwrap();
        let b_addr = tree
            .get_registry_address_by_script_hash(
                gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID,
                &script_hash_b,
            )
            .unwrap()
            .unwrap();
        let balance_a = tree.get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &a_addr).unwrap();
        let balance_b = tree.get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &b_addr).unwrap();
        assert_eq!(balance_a, CKBCapacity::from_layer1(800 * CKB).to_layer2());
        assert_eq!(balance_b, CKBCapacity::from_layer1(500 * CKB).to_layer2());
    }

    drop(chain2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_rewind_to_last_valid_tip_just_after_bad_block_reverted() {
    let rollup_type_script = Script::default();
    let rollup_script_hash = rollup_type_script.hash();
    let mut chain = setup_chain(rollup_type_script.clone()).await;
    let rollup_cell = CellOutput::new_builder()
        .type_(Some(rollup_type_script.clone()).pack())
        .build();

    // update block 1
    let alice_script = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.pack())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.to_vec();
            args.extend(&[42u8; 20]);
            args.pack()
        })
        .build();
    let deposit = DepositRequest::new_builder()
        .capacity((4000u64 * CKB).pack())
        .script(alice_script.clone())
        .registry_id(gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID.pack())
        .build();
    let deposit_info_vec = DepositInfoVec::new_builder()
        .push(into_deposit_info_cell(chain.generator().rollup_context(), deposit).pack())
        .build();
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(&chain, &mut mem_pool, deposit_info_vec.clone())
            .await
            .unwrap()
    };
    let action1 = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block: block_result.block.clone(),
            deposit_info_vec,
            deposit_asset_scripts: Default::default(),
            withdrawals: Default::default(),
        },
        transaction: build_sync_tx(rollup_cell.clone(), block_result),
    };
    let param = SyncParam {
        updates: vec![action1],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    chain.notify_new_tip().await.unwrap();
    assert!(chain.last_sync_event().is_success());

    // with for deposit finalize
    for _ in 0..DEFAULT_FINALITY_BLOCKS {
        produce_empty_block(&mut chain, rollup_cell.clone()).await;
    }

    // update bad block
    let withdrawal = {
        let owner_lock = Script::default();
        let raw = RawWithdrawalRequest::new_builder()
            .capacity((1000 * CKB).pack())
            .account_script_hash(alice_script.hash().pack())
            .sudt_script_hash(H256::zero().pack())
            .owner_lock_hash(owner_lock.hash().pack())
            .registry_id(gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID.pack())
            .chain_id(crate::testing_tool::chain::TEST_CHAIN_ID.pack())
            .build();
        let withdrawal = WithdrawalRequest::new_builder().raw(raw).build();
        WithdrawalRequestExtra::new_builder()
            .request(withdrawal)
            .owner_lock(owner_lock)
            .build()
    };
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        mem_pool.push_withdrawal_request(withdrawal).await.unwrap();
        construct_block(&chain, &mut mem_pool, Default::default())
            .await
            .unwrap()
    };
    let mut bad_block_result = {
        let ProduceBlockResult {
            block,
            global_state,
            withdrawal_extras,
            deposit_cells,
            remaining_capacity,
        } = block_result.clone();
        let (bad_block, bad_global_state) =
            generate_bad_block_using_first_withdrawal(&chain, block, global_state);
        let withdrawal_extras = withdrawal_extras
            .into_iter()
            .enumerate()
            .map(|(i, withdraw)| {
                withdraw
                    .as_builder()
                    .request(bad_block.withdrawals().get(i).unwrap())
                    .build()
            })
            .collect();
        ProduceBlockResult {
            block: bad_block,
            global_state: bad_global_state,
            withdrawal_extras,
            deposit_cells,
            remaining_capacity,
        }
    };

    let update_bad_block = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block: bad_block_result.block.clone(),
            deposit_info_vec: Default::default(),
            deposit_asset_scripts: Default::default(),
            withdrawals: bad_block_result.withdrawal_extras.clone(),
        },
        transaction: build_sync_tx(rollup_cell.clone(), bad_block_result.clone()),
    };
    let param = SyncParam {
        updates: vec![update_bad_block],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    chain.notify_new_tip().await.unwrap();
    assert!(matches!(
        chain.last_sync_event(),
        SyncEvent::BadBlock { .. }
    ));

    let tip_block = chain.store().get_tip_block().unwrap();
    let tip_block_number: u64 = tip_block.raw().number().unpack();
    assert_eq!(tip_block_number, 8);

    // challenge bad block
    let challenge_context = match chain.last_sync_event() {
        SyncEvent::BadBlock { context } => context.to_owned(),
        _ => unreachable!(),
    };
    let challenge_cell = ChallengeCell {
        input: CellInput::default(),
        output: CellOutput::default(),
        output_data: Bytes::default(),
    };

    bad_block_result.global_state = bad_block_result
        .global_state
        .as_builder()
        .status(Status::Halting.into())
        .build();

    let challenge_bad_block = L1Action {
        context: L1ActionContext::Challenge {
            cell: challenge_cell,
            target: challenge_context.target,
            witness: challenge_context.witness,
        },
        transaction: build_sync_tx(rollup_cell.clone(), bad_block_result.clone()),
    };
    let param = SyncParam {
        updates: vec![challenge_bad_block],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    chain.notify_new_tip().await.unwrap();
    assert!(matches!(
        chain.last_sync_event(),
        SyncEvent::WaitChallenge { .. }
    ));

    // Revert bad block
    let reverted_block_smt_root = match chain.last_sync_event() {
        SyncEvent::WaitChallenge { cell: _, context } => context.post_reverted_block_root,
        _ => unreachable!(),
    };
    let db = &chain.store().begin_transaction();
    let last_valid_tip_block_hash = db.get_last_valid_tip_block_hash().unwrap();
    let last_valid_tip_block = db.get_last_valid_tip_block().unwrap();
    let block_smt = {
        let global_state = db
            .get_block_post_global_state(&last_valid_tip_block_hash)
            .unwrap();
        global_state.unwrap().block()
    };
    let reverted_block_result = ProduceBlockResult {
        global_state: bad_block_result
            .global_state
            .as_builder()
            .status(Status::Running.into())
            .reverted_block_root(reverted_block_smt_root.pack())
            .tip_block_hash(last_valid_tip_block_hash.pack())
            .block(block_smt)
            .account(last_valid_tip_block.raw().post_account())
            .build(),
        ..bad_block_result
    };

    let revert_bad_block = L1Action {
        context: L1ActionContext::Revert {
            reverted_blocks: vec![reverted_block_result.block.raw()],
        },
        transaction: build_sync_tx(rollup_cell.clone(), reverted_block_result),
    };
    let param = SyncParam {
        updates: vec![revert_bad_block],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    assert!(chain.last_sync_event().is_success());

    let local_reverted_block_smt_root = db.get_reverted_block_smt_root().unwrap();
    assert_eq!(local_reverted_block_smt_root, reverted_block_smt_root);

    //  Rewind to last tip
    //  IMPORTANT: simulate restart process
    let mut chain = restart_chain(&chain, rollup_type_script, None).await;
    let last_valid_tip_global_state = db
        .get_block_post_global_state(&last_valid_tip_block_hash)
        .unwrap();
    let rewind = RevertedL1Action {
        prev_global_state: last_valid_tip_global_state.clone().unwrap(),
        context: RevertL1ActionContext::RewindToLastValidTip,
    };
    let param = SyncParam {
        reverts: vec![rewind],
        updates: vec![],
    };
    chain.sync(param).await.unwrap();

    let local_reverted_block_smt_root = db.get_reverted_block_smt_root().unwrap();
    assert_eq!(
        local_reverted_block_smt_root,
        Unpack::<H256>::unpack(&last_valid_tip_global_state.unwrap().reverted_block_root())
    );

    // Produce new block
    let bob_script = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.pack())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.to_vec();
            args.extend(&[43u8; 20]);
            args.pack()
        })
        .build();
    let deposit = DepositRequest::new_builder()
        .capacity((500u64 * CKB).pack())
        .script(bob_script)
        .registry_id(gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID.pack())
        .build();
    let deposit_info_vec = DepositInfoVec::new_builder()
        .push(into_deposit_info_cell(chain.generator().rollup_context(), deposit).pack())
        .build();
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(&chain, &mut mem_pool, deposit_info_vec.clone())
            .await
            .unwrap()
    };
    let new_block = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block: block_result.block.clone(),
            deposit_info_vec,
            deposit_asset_scripts: Default::default(),
            withdrawals: block_result.withdrawal_extras.clone(),
        },
        transaction: build_sync_tx(rollup_cell, block_result),
    };
    let param = SyncParam {
        updates: vec![new_block],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    chain.notify_new_tip().await.unwrap();
    assert!(chain.last_sync_event().is_success());

    let tip_block = chain.store().get_tip_block().unwrap();
    let tip_block_number: u64 = tip_block.raw().number().unpack();
    assert_eq!(tip_block_number, 8);
}

async fn produce_empty_block(chain: &mut Chain, rollup_cell: CellOutput) {
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(chain, &mut mem_pool, Default::default())
            .await
            .unwrap()
    };

    let l2block = block_result.block.clone();
    let transaction = build_sync_tx(rollup_cell, block_result);

    let update = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block,
            deposit_info_vec: Default::default(),
            deposit_asset_scripts: Default::default(),
            withdrawals: Default::default(),
        },
        transaction,
    };
    let param = SyncParam {
        updates: vec![update],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    chain.notify_new_tip().await.unwrap();
    assert!(chain.last_sync_event().is_success());
}
