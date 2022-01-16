use crate::testing_tool::chain::{
    build_sync_tx, construct_block, restart_chain, setup_chain, ALWAYS_SUCCESS_CODE_HASH,
    DEFAULT_FINALITY_BLOCKS,
};

use gw_block_producer::produce_block::ProduceBlockResult;
use gw_chain::chain::{
    Chain, ChallengeCell, L1Action, L1ActionContext, RevertL1ActionContext, RevertedL1Action,
    SyncEvent, SyncParam,
};
use gw_common::{
    builtins::CKB_SUDT_ACCOUNT_ID,
    h256_ext::H256Ext,
    merkle_utils::{calculate_ckb_merkle_root, ckb_merkle_leaf_hash},
    smt::Blake2bHasher,
    state::{to_short_address, State},
    H256,
};
use gw_store::{state::state_db::StateContext, traits::chain_store::ChainStore};
use gw_types::{
    bytes::Bytes,
    core::{ScriptHashType, Status},
    packed::{
        BlockMerkleState, CellInput, CellOutput, DepositRequest, GlobalState, L2Block,
        L2BlockCommittedInfo, RawWithdrawalRequest, Script, SubmitWithdrawals, WithdrawalRequest,
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
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(chain, &mut mem_pool, vec![deposit.clone()])
            .await
            .unwrap()
    };
    let l2block = block_result.block.clone();
    let transaction = build_sync_tx(rollup_cell, block_result);
    let l2block_committed_info = L2BlockCommittedInfo::new_builder()
        .number(expected_tip.pack())
        .build();

    let update = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block,
            deposit_requests: vec![deposit],
            deposit_asset_scripts: Default::default(),
        },
        transaction,
        l2block_committed_info,
    };
    let param = SyncParam {
        updates: vec![update],
        reverts: Default::default(),
    };
    chain.sync(param.clone()).await.unwrap();
    assert!(chain.last_sync_event().is_success());

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

#[tokio::test]
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
            args.push(42);
            args.pack()
        })
        .build();
    let deposit = DepositRequest::new_builder()
        .capacity((290u64 * CKB).pack())
        .script(user_script_a.clone())
        .build();
    produce_a_block(&mut chain, deposit, rollup_cell.clone(), 1).await;

    // block #2
    let deposit = DepositRequest::new_builder()
        .capacity((400u64 * CKB).pack())
        .script(user_script_a.clone())
        .build();
    produce_a_block(&mut chain, deposit, rollup_cell.clone(), 2).await;

    // block #3
    let user_script_b = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.clone().pack())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.to_vec();
            args.push(50);
            args.pack()
        })
        .build();
    let deposit = DepositRequest::new_builder()
        .capacity((500u64 * CKB).pack())
        .script(user_script_b.clone())
        .build();
    produce_a_block(&mut chain, deposit, rollup_cell, 3).await;

    // check state
    {
        let db = chain.store().begin_transaction();
        let tree = db.state_tree(StateContext::ReadOnly).unwrap();
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
        // 0 is meta contract, 1 is ckb sudt, so the user id start from 2
        assert_eq!(id_a, 2);
        assert_eq!(id_b, 3);
        let balance_a = tree
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, to_short_address(&script_hash_a))
            .unwrap();
        let balance_b = tree
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, to_short_address(&script_hash_b))
            .unwrap();
        assert_eq!(balance_a, 690 * CKB as u128);
        assert_eq!(balance_b, 500 * CKB as u128);
    }

    drop(chain);
}

#[tokio::test]
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
                args.push(7);
                args.pack()
            })
            .build();
        let deposit = DepositRequest::new_builder()
            .capacity((290u64 * CKB).pack())
            .script(charlie_script)
            .build();
        let chain = setup_chain(rollup_type_script).await;
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        let block_result = construct_block(&chain, &mut mem_pool, vec![deposit.clone()])
            .await
            .unwrap();

        L1Action {
            context: L1ActionContext::SubmitBlock {
                l2block: block_result.block.clone(),
                deposit_requests: vec![deposit],
                deposit_asset_scripts: Default::default(),
            },
            transaction: build_sync_tx(rollup_cell.clone(), block_result),
            l2block_committed_info: L2BlockCommittedInfo::new_builder()
                .number(1u64.pack())
                .build(),
        }
    };
    // update block 1
    let alice_script = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.clone().pack())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.to_vec();
            args.push(42);
            args.pack()
        })
        .build();
    let deposit = DepositRequest::new_builder()
        .capacity((400u64 * CKB).pack())
        .script(alice_script)
        .build();
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(&chain, &mut mem_pool, vec![deposit.clone()])
            .await
            .unwrap()
    };
    let action1 = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block: block_result.block.clone(),
            deposit_requests: vec![deposit],
            deposit_asset_scripts: Default::default(),
        },
        transaction: build_sync_tx(rollup_cell.clone(), block_result),
        l2block_committed_info: L2BlockCommittedInfo::new_builder()
            .number(1u64.pack())
            .build(),
    };
    let param = SyncParam {
        updates: vec![action1],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    assert!(chain.last_sync_event().is_success());
    // update block 2
    let bob_script = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.clone().pack())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.to_vec();
            args.push(43);
            args.pack()
        })
        .build();
    let deposit = DepositRequest::new_builder()
        .capacity((500u64 * CKB).pack())
        .script(bob_script)
        .build();
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(&chain, &mut mem_pool, vec![deposit.clone()])
            .await
            .unwrap()
    };
    let action2 = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block: block_result.block.clone(),
            deposit_requests: vec![deposit],
            deposit_asset_scripts: Default::default(),
        },
        transaction: build_sync_tx(rollup_cell, block_result),
        l2block_committed_info: L2BlockCommittedInfo::new_builder()
            .number(2u64.pack())
            .build(),
    };
    let param = SyncParam {
        updates: vec![action2],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    assert!(chain.last_sync_event().is_success());
    let tip_block = chain.store().get_tip_block().unwrap();
    let tip_block_number: u64 = tip_block.raw().number().unpack();
    assert_eq!(tip_block_number, 2);

    // revert blocks
    let db = chain.store().begin_transaction();
    let tip_block_parent_hash: H256 = tip_block.raw().parent_block_hash().unpack();
    let revert_action2 = {
        let prev_global_state = db
            .get_block_post_global_state(&tip_block_parent_hash)
            .unwrap()
            .unwrap();
        let l2block_committed_info = db
            .get_l2block_committed_info(&tip_block_parent_hash)
            .unwrap()
            .unwrap();
        let context = RevertL1ActionContext::SubmitValidBlock { l2block: tip_block };
        RevertedL1Action {
            prev_global_state,
            l2block_committed_info,
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
        let l2block_committed_info = db
            .get_l2block_committed_info(&tip_grandpa_block_hash)
            .unwrap()
            .unwrap();
        let context = RevertL1ActionContext::SubmitValidBlock {
            l2block: tip_parent_block,
        };
        RevertedL1Action {
            prev_global_state,
            l2block_committed_info,
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
        let tree = db.state_tree(StateContext::ReadOnly).unwrap();
        let current_account_root = tree.calculate_root().unwrap();
        let expected_account_root: H256 = tip_block.raw().post_account().merkle_root().unpack();
        assert_eq!(
            current_account_root, expected_account_root,
            "check account tree"
        );
    }
}

#[tokio::test]
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
        .expect("get default EoA hash");

    // update block 1
    let alice_script = Script::new_builder()
        .code_hash(default_eoa_code_hash.clone())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.to_vec();
            args.push(42);
            args.pack()
        })
        .build();
    let deposit = DepositRequest::new_builder()
        .capacity((400u64 * CKB).pack())
        .script(alice_script.clone())
        .build();
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(&chain, &mut mem_pool, vec![deposit.clone()])
            .await
            .unwrap()
    };
    let action1 = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block: block_result.block.clone(),
            deposit_requests: vec![deposit],
            deposit_asset_scripts: Default::default(),
        },
        transaction: build_sync_tx(rollup_cell.clone(), block_result),
        l2block_committed_info: L2BlockCommittedInfo::new_builder()
            .number(1u64.pack())
            .build(),
    };
    let param = SyncParam {
        updates: vec![action1],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    assert!(chain.last_sync_event().is_success());
    // update block 2
    let bob_script = Script::new_builder()
        .code_hash(default_eoa_code_hash)
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.to_vec();
            args.push(43);
            args.pack()
        })
        .build();
    let deposit = DepositRequest::new_builder()
        .capacity((500u64 * CKB).pack())
        .script(bob_script.clone())
        .build();
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(&chain, &mut mem_pool, vec![deposit.clone()])
            .await
            .unwrap()
    };
    let action2 = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block: block_result.block.clone(),
            deposit_requests: vec![deposit],
            deposit_asset_scripts: Default::default(),
        },
        transaction: build_sync_tx(rollup_cell, block_result),
        l2block_committed_info: L2BlockCommittedInfo::new_builder()
            .number(2u64.pack())
            .build(),
    };
    let param = SyncParam {
        updates: vec![action2.clone()],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
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
                l2block_committed_info,
                context,
            } = action;
            let l2block = match context {
                L1ActionContext::SubmitBlock { l2block, .. } => l2block,
                _ => unreachable!(),
            };
            let context = RevertL1ActionContext::SubmitValidBlock { l2block };
            RevertedL1Action {
                prev_global_state,
                l2block_committed_info,
                context,
            }
        })
        .collect::<Vec<_>>();

    let param = SyncParam {
        updates: Default::default(),
        reverts,
    };
    chain.sync(param).await.unwrap();
    assert!(chain.last_sync_event().is_success());

    let tip_block = chain.store().get_tip_block().unwrap();
    let tip_block_number: u64 = tip_block.raw().number().unpack();
    assert_eq!(tip_block_number, 1);

    // check account SMT, should be able to calculate account state root
    {
        let db = chain.store().begin_transaction();
        let tree = db.state_tree(StateContext::ReadOnly).unwrap();
        let current_account_root = tree.calculate_root().unwrap();
        let expected_account_root: H256 = tip_block.raw().post_account().merkle_root().unpack();
        assert_eq!(
            current_account_root, expected_account_root,
            "check account tree"
        );

        assert_eq!(tree.get_account_count().unwrap(), 3);
        let alice_script_hash: H256 = alice_script.hash().into();
        let alice_id = tree
            .get_account_id_by_script_hash(&alice_script_hash)
            .unwrap()
            .unwrap();
        assert_eq!(alice_id, 2);
        let alice_balance = tree
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, to_short_address(&alice_script_hash))
            .unwrap();
        assert_eq!(alice_balance, 400 * CKB as u128);

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
    assert!(chain.last_sync_event().is_success());

    // check block2 agnain

    let tip_block = chain.store().get_tip_block().unwrap();
    let tip_block_number: u64 = tip_block.raw().number().unpack();
    assert_eq!(tip_block_number, 2);

    {
        let db = chain.store().begin_transaction();
        let tree = db.state_tree(StateContext::ReadOnly).unwrap();
        let current_account_root = tree.calculate_root().unwrap();
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
        assert_eq!(alice_id, 2);
        let alice_balance = tree
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, to_short_address(&alice_script_hash))
            .unwrap();
        assert_eq!(alice_balance, 400 * CKB as u128);

        let bob_script_hash: H256 = bob_script.hash().into();
        let bob_id = tree
            .get_account_id_by_script_hash(&bob_script_hash)
            .unwrap()
            .unwrap();
        assert_eq!(bob_id, 3);

        let bob_balance = tree
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, to_short_address(&bob_script_hash))
            .unwrap();
        assert_eq!(bob_balance, 500 * CKB as u128);
    }
}

#[tokio::test]
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
            args.push(42);
            args.pack()
        })
        .build();
    let sudt_script_hash: H256 = [42u8; 32].into();
    let deposit = DepositRequest::new_builder()
        .capacity((400u64 * CKB).pack())
        .script(user_script_a.clone())
        .sudt_script_hash(sudt_script_hash.pack())
        .build();
    let sync_1 = produce_a_block(&mut chain1, deposit, rollup_cell.clone(), 1).await;

    // block #2
    let deposit = DepositRequest::new_builder()
        .capacity((400u64 * CKB).pack())
        .script(user_script_a.clone())
        .build();
    let sync_2 = produce_a_block(&mut chain1, deposit, rollup_cell.clone(), 2).await;

    // block #3
    let user_script_b = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.clone().pack())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.to_vec();
            args.push(50);
            args.pack()
        })
        .build();
    let deposit = DepositRequest::new_builder()
        .capacity((500u64 * CKB).pack())
        .script(user_script_b.clone())
        .sudt_script_hash(sudt_script_hash.pack())
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
        let db = chain2.store().begin_transaction();
        let tip_block = db.get_tip_block().unwrap();
        let tip_block_number: u64 = tip_block.raw().number().unpack();
        let tip_block_hash = db.get_tip_block_hash().unwrap();
        assert_eq!(tip_block_hash, tip_block.hash().into());
        assert_eq!(tip_block_number, 3);

        let tree = db.state_tree(StateContext::ReadOnly).unwrap();
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
        // 0 is meta contract, 1 is ckb sudt, so the user id start from 2
        assert_eq!(id_a, 2);
        assert_eq!(id_b, 4);
        let balance_a = tree
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, to_short_address(&script_hash_a))
            .unwrap();
        let balance_b = tree
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, to_short_address(&script_hash_b))
            .unwrap();
        assert_eq!(balance_a, 800 * CKB as u128);
        assert_eq!(balance_b, 500 * CKB as u128);
    }

    drop(chain2);
}

#[tokio::test]
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
            args.push(42);
            args.pack()
        })
        .build();
    let deposit = DepositRequest::new_builder()
        .capacity((4000u64 * CKB).pack())
        .script(alice_script.clone())
        .build();
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(&chain, &mut mem_pool, vec![deposit.clone()])
            .await
            .unwrap()
    };
    let action1 = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block: block_result.block.clone(),
            deposit_requests: vec![deposit],
            deposit_asset_scripts: Default::default(),
        },
        transaction: build_sync_tx(rollup_cell.clone(), block_result),
        l2block_committed_info: L2BlockCommittedInfo::new_builder()
            .number(1u64.pack())
            .build(),
    };
    let param = SyncParam {
        updates: vec![action1],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    assert!(chain.last_sync_event().is_success());

    // with for deposit finalize
    for _ in 0..DEFAULT_FINALITY_BLOCKS {
        produce_empty_block(&mut chain, rollup_cell.clone()).await;
    }

    // update bad block
    let withdrawal = {
        let raw = RawWithdrawalRequest::new_builder()
            .capacity((1000 * CKB).pack())
            .account_script_hash(alice_script.hash().pack())
            .sudt_script_hash(H256::zero().pack())
            .build();
        WithdrawalRequest::new_builder().raw(raw).build()
    };
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        mem_pool
            .push_withdrawal_request(withdrawal.into())
            .await
            .unwrap();
        construct_block(&chain, &mut mem_pool, Vec::default())
            .await
            .unwrap()
    };
    let bad_block_result = {
        let ProduceBlockResult {
            block,
            global_state,
            withdrawal_extras,
        } = block_result;
        let (bad_block, bad_global_state) = generate_bad_block(&chain, block, global_state);
        ProduceBlockResult {
            block: bad_block,
            global_state: bad_global_state,
            withdrawal_extras,
        }
    };

    let update_bad_block = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block: bad_block_result.block.clone(),
            deposit_requests: vec![],
            deposit_asset_scripts: Default::default(),
        },
        transaction: build_sync_tx(rollup_cell.clone(), bad_block_result.clone()),
        l2block_committed_info: L2BlockCommittedInfo::new_builder()
            .number((DEFAULT_FINALITY_BLOCKS + 2u64).pack())
            .build(),
    };
    let param = SyncParam {
        updates: vec![update_bad_block],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
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

    let bad_block_result = {
        let ProduceBlockResult {
            block,
            global_state,
            withdrawal_extras,
        } = bad_block_result;

        let global_state = global_state
            .as_builder()
            .status(Status::Halting.into())
            .build();

        ProduceBlockResult {
            global_state,
            block,
            withdrawal_extras,
        }
    };

    let challenge_bad_block = L1Action {
        context: L1ActionContext::Challenge {
            cell: challenge_cell,
            target: challenge_context.target,
            witness: challenge_context.witness,
        },
        transaction: build_sync_tx(rollup_cell.clone(), bad_block_result.clone()),
        l2block_committed_info: L2BlockCommittedInfo::new_builder()
            .number((DEFAULT_FINALITY_BLOCKS + 3u64).pack())
            .build(),
    };
    let param = SyncParam {
        updates: vec![challenge_bad_block],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    assert!(matches!(
        chain.last_sync_event(),
        SyncEvent::WaitChallenge { .. }
    ));

    // Revert bad block
    let reverted_block_smt_root = match chain.last_sync_event() {
        SyncEvent::WaitChallenge { cell: _, context } => context.post_reverted_block_root,
        _ => unreachable!(),
    };
    let db = chain.store().begin_transaction();
    let last_valid_tip_block_hash = db.get_last_valid_tip_block_hash().unwrap();
    let last_valid_tip_block = db.get_last_valid_tip_block().unwrap();
    let block_smt = {
        let global_state = db
            .get_block_post_global_state(&last_valid_tip_block_hash)
            .unwrap();
        global_state.unwrap().block()
    };
    let reverted_block_result = {
        let ProduceBlockResult {
            block,
            global_state,
            withdrawal_extras,
        } = bad_block_result;

        let global_state = global_state
            .as_builder()
            .status(Status::Running.into())
            .reverted_block_root(reverted_block_smt_root.pack())
            .tip_block_hash(last_valid_tip_block_hash.pack())
            .block(block_smt)
            .account(last_valid_tip_block.raw().post_account())
            .build();

        ProduceBlockResult {
            global_state,
            block,
            withdrawal_extras,
        }
    };

    let revert_bad_block = L1Action {
        context: L1ActionContext::Revert {
            reverted_blocks: vec![reverted_block_result.block.raw()],
        },
        transaction: build_sync_tx(rollup_cell.clone(), reverted_block_result),
        l2block_committed_info: L2BlockCommittedInfo::new_builder()
            .number((DEFAULT_FINALITY_BLOCKS + 3u64).pack())
            .build(),
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
    let last_valid_tip_committed_info = db
        .get_l2block_committed_info(&last_valid_tip_block_hash)
        .unwrap();
    let rewind = RevertedL1Action {
        prev_global_state: last_valid_tip_global_state.clone().unwrap(),
        l2block_committed_info: last_valid_tip_committed_info.unwrap(),
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
            args.push(43);
            args.pack()
        })
        .build();
    let deposit = DepositRequest::new_builder()
        .capacity((500u64 * CKB).pack())
        .script(bob_script)
        .build();
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(&chain, &mut mem_pool, vec![deposit.clone()])
            .await
            .unwrap()
    };
    let new_block = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block: block_result.block.clone(),
            deposit_requests: vec![deposit],
            deposit_asset_scripts: Default::default(),
        },
        transaction: build_sync_tx(rollup_cell, block_result),
        l2block_committed_info: L2BlockCommittedInfo::new_builder()
            .number((DEFAULT_FINALITY_BLOCKS + 4u64).pack())
            .build(),
    };
    let param = SyncParam {
        updates: vec![new_block],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
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
    let db = chain.store().begin_transaction();
    let tip_block_hash = db.get_tip_block_hash().unwrap();
    let tip_committed_info = db.get_l2block_committed_info(&tip_block_hash).unwrap();
    let l1_number = tip_committed_info.unwrap().number().unpack();

    let l2block = block_result.block.clone();
    let transaction = build_sync_tx(rollup_cell, block_result);
    let l2block_committed_info = L2BlockCommittedInfo::new_builder()
        .number((l1_number + 1).pack())
        .build();

    let update = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block,
            deposit_requests: Default::default(),
            deposit_asset_scripts: Default::default(),
        },
        transaction,
        l2block_committed_info,
    };
    let param = SyncParam {
        updates: vec![update],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    assert!(chain.last_sync_event().is_success());
}

fn generate_bad_block(
    chain: &Chain,
    block: L2Block,
    global_state: GlobalState,
) -> (L2Block, GlobalState) {
    let block = {
        let withdrawal = block.withdrawals().get_unchecked(0);
        let raw_withdrawal = withdrawal
            .raw()
            .as_builder()
            .account_script_hash([9u8; 32].pack())
            .build();
        let bad_withdrawal = withdrawal.as_builder().raw(raw_withdrawal).build();

        let mut withdrawals: Vec<WithdrawalRequest> = block.withdrawals().into_iter().collect();
        *withdrawals.get_mut(0).expect("exists") = bad_withdrawal;

        let withdrawal_witness_root = {
            let witnesses = withdrawals
                .iter()
                .enumerate()
                .map(|(idx, t)| ckb_merkle_leaf_hash(idx as u32, &t.witness_hash().into()));
            calculate_ckb_merkle_root(witnesses.collect()).unwrap()
        };

        let submit_withdrawals = SubmitWithdrawals::new_builder()
            .withdrawal_witness_root(withdrawal_witness_root.pack())
            .withdrawal_count((withdrawals.len() as u32).pack())
            .build();

        let raw_block = block
            .raw()
            .as_builder()
            .submit_withdrawals(submit_withdrawals)
            .build();

        block
            .as_builder()
            .raw(raw_block)
            .withdrawals(withdrawals.pack())
            .build()
    };

    let block_number = block.raw().number().unpack();
    let global_state = {
        let db = chain.store().begin_transaction();

        let bad_block_proof = db
            .block_smt()
            .unwrap()
            .merkle_proof(vec![H256::from_u64(block_number)])
            .unwrap()
            .compile(vec![(H256::from_u64(block_number), H256::zero())])
            .unwrap();

        // Generate new block smt for global state
        let bad_block_smt = {
            let bad_block_root: [u8; 32] = bad_block_proof
                .compute_root::<Blake2bHasher>(vec![(block.smt_key().into(), block.hash().into())])
                .unwrap()
                .into();

            BlockMerkleState::new_builder()
                .merkle_root(bad_block_root.pack())
                .count((block_number + 1).pack())
                .build()
        };

        global_state
            .as_builder()
            .block(bad_block_smt)
            .tip_block_hash(block.hash().pack())
            .build()
    };

    (block, global_state)
}
