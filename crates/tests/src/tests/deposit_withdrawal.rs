#![allow(clippy::mutable_key_type)]

use crate::testing_tool::chain::{
    apply_block_result, construct_block, construct_block_with_timestamp, into_deposit_info_cell,
    produce_empty_block, setup_chain, ALWAYS_SUCCESS_CODE_HASH, DEFAULT_FINALITY_BLOCKS,
};

use anyhow::Result;
use gw_chain::chain::{Chain, RevertL1ActionContext, RevertedL1Action};
use gw_common::{
    builtins::{CKB_SUDT_ACCOUNT_ID, ETH_REGISTRY_ACCOUNT_ID},
    ckb_decimal::CKBCapacity,
    state::State,
    H256,
};
use gw_generator::{
    error::{DepositError, WithdrawalError},
    sudt::build_l2_sudt_script,
    Error,
};
use gw_store::{state::state_db::StateContext, traits::chain_store::ChainStore};
use gw_types::{
    core::ScriptHashType,
    packed::{
        DepositInfoVec, DepositRequest, RawWithdrawalRequest, Script, WithdrawalRequest,
        WithdrawalRequestExtra,
    },
    prelude::*,
    U256,
};

use std::{collections::HashSet, iter::FromIterator};

/// Deposit, produce new block and update chain.
async fn deposite_to_chain(
    chain: &mut Chain,
    user_script: Script,
    capacity: u64,
    sudt_script_hash: H256, // To allow deposit ckb only
    sudt_script: Script,
    amount: u128,
) -> Result<()> {
    let deposit = DepositRequest::new_builder()
        .capacity(capacity.pack())
        .sudt_script_hash(sudt_script_hash.pack())
        .amount(amount.pack())
        .script(user_script)
        .registry_id(gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID.pack())
        .build();
    let deposit_info_vec = DepositInfoVec::new_builder()
        .push(into_deposit_info_cell(chain.generator().rollup_context(), deposit).pack())
        .build();

    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(chain, &mut mem_pool, deposit_info_vec.clone()).await?
    };
    let asset_scripts = if sudt_script_hash == H256::zero() {
        HashSet::new()
    } else {
        HashSet::from_iter(vec![sudt_script])
    };

    // deposit
    apply_block_result(chain, block_result, deposit_info_vec, asset_scripts).await;
    Ok(())
}

async fn withdrawal_from_chain(
    chain: &mut Chain,
    user_script_hash: H256,
    capacity: u64,
    sudt_script_hash: H256,
    amount: u128,
) -> Result<()> {
    let withdrawal = {
        let owner_lock = Script::default();
        let raw = RawWithdrawalRequest::new_builder()
            .capacity(capacity.pack())
            .account_script_hash(user_script_hash.pack())
            .sudt_script_hash(sudt_script_hash.pack())
            .amount(amount.pack())
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
        mem_pool.push_withdrawal_request(withdrawal).await?;
        construct_block(chain, &mut mem_pool, Default::default())
            .await
            .unwrap()
    };

    // deposit
    apply_block_result(chain, block_result, Default::default(), HashSet::new()).await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_deposit_and_withdrawal() {
    let rollup_type_script = Script::default();
    let rollup_script_hash = rollup_type_script.hash();
    let mut chain = setup_chain(rollup_type_script.clone()).await;
    let capacity = 600_00000000;
    let user_script = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.pack())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.to_vec();
            args.extend(&[42u8; 20]);
            args.pack()
        })
        .build();
    let user_script_hash = user_script.hash();
    // deposit
    deposite_to_chain(
        &mut chain,
        user_script,
        capacity,
        H256::zero(),
        Script::default(),
        0,
    )
    .await
    .unwrap();
    let (user_id, user_script_hash, user_addr, ckb_balance, ckb_total_supply) = {
        let mem_pool = chain.mem_pool().as_ref().unwrap().lock().await;
        let snap = mem_pool.mem_pool_state().load();
        let tree = snap.state().unwrap();
        // check user account
        assert_eq!(
            tree.get_account_count().unwrap(),
            4,
            "3 builtin accounts plus 1 deposit"
        );
        let user_id = tree
            .get_account_id_by_script_hash(&user_script_hash.into())
            .unwrap()
            .expect("account exists");
        assert_ne!(user_id, 0);
        let user_script_hash = tree.get_script_hash(user_id).unwrap();
        let user_addr = tree
            .get_registry_address_by_script_hash(ETH_REGISTRY_ACCOUNT_ID, &user_script_hash)
            .unwrap()
            .unwrap();
        let ckb_balance = tree
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &user_addr)
            .unwrap();
        assert_eq!(ckb_balance, CKBCapacity::from_layer1(capacity).to_layer2());
        let ckb_total_supply = tree.get_sudt_total_supply(CKB_SUDT_ACCOUNT_ID).unwrap();
        assert_eq!(
            ckb_total_supply,
            CKBCapacity::from_layer1(capacity).to_layer2()
        );
        (
            user_id,
            user_script_hash,
            user_addr,
            ckb_balance,
            ckb_total_supply,
        )
    };

    // wait for deposit finalize
    for _ in 0..DEFAULT_FINALITY_BLOCKS {
        produce_empty_block(&mut chain).await.unwrap();
    }

    // Check remaining ckb capacity.
    let tip = chain.local_state().tip().raw().number().unpack();
    let cap = chain
        .store()
        .get_block_post_finalized_custodian_capacity(tip)
        .unwrap();
    // Tip block should have 0 capacity. Next block can collect finalized deposit capacity.
    assert_eq!(cap.capacity().unpack(), 0);

    // check tx pool state
    {
        let mem_pool = chain.mem_pool().as_ref().unwrap().lock().await;
        let snap = mem_pool.mem_pool_state().load();
        let state = snap.state().unwrap();
        assert_eq!(
            state
                .get_account_id_by_script_hash(&user_script_hash)
                .unwrap()
                .unwrap(),
            user_id
        );
        assert_eq!(
            state
                .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &user_addr)
                .unwrap(),
            CKBCapacity::from_layer1(capacity).to_layer2()
        );
        assert_eq!(
            state.get_sudt_total_supply(CKB_SUDT_ACCOUNT_ID,).unwrap(),
            CKBCapacity::from_layer1(capacity).to_layer2()
        )
    }
    // withdrawal
    let withdraw_capacity = 322_00000000u64;
    withdrawal_from_chain(
        &mut chain,
        user_script_hash,
        withdraw_capacity,
        H256::zero(),
        0,
    )
    .await
    .unwrap();
    // check status

    // Check remaining ckb capacity.
    let tip = chain.local_state().tip().raw().number().unpack();
    let cap = chain
        .store()
        .get_block_post_finalized_custodian_capacity(tip)
        .unwrap();
    assert_eq!(
        cap.capacity().unpack(),
        (capacity - withdraw_capacity).into()
    );

    let db = chain.store().begin_transaction();
    let tree = db.state_tree(StateContext::ReadOnly).unwrap();
    let ckb_balance2 = tree
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &user_addr)
        .unwrap();
    assert_eq!(
        ckb_balance,
        ckb_balance2 + CKBCapacity::from_layer1(withdraw_capacity).to_layer2()
    );
    let ckb_total_supply2 = tree.get_sudt_total_supply(CKB_SUDT_ACCOUNT_ID).unwrap();
    assert_eq!(
        ckb_total_supply,
        ckb_total_supply2 + CKBCapacity::from_layer1(withdraw_capacity).to_layer2()
    );
    let nonce = tree.get_nonce(user_id).unwrap();
    assert_eq!(nonce, 1);
    // check tx pool state
    {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mem_pool = mem_pool.lock().await;
        let snap = mem_pool.mem_pool_state().load();
        let state = snap.state().unwrap();
        assert_eq!(
            state
                .get_account_id_by_script_hash(&user_script_hash)
                .unwrap()
                .unwrap(),
            user_id
        );
        assert_eq!(
            state
                .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &user_addr)
                .unwrap(),
            ckb_balance2
        );
        assert_eq!(
            state.get_sudt_total_supply(CKB_SUDT_ACCOUNT_ID,).unwrap(),
            ckb_balance2
        );
        assert_eq!(state.get_nonce(user_id).unwrap(), nonce);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_deposit_u128_overflow() {
    let rollup_type_script = Script::default();
    let rollup_script_hash = rollup_type_script.hash();
    let mut chain = setup_chain(rollup_type_script.clone()).await;

    let sudt_script = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.pack())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.to_vec();
            args.push(77);
            args.pack()
        })
        .build();
    let sudt_script_hash: H256 = sudt_script.hash().into();

    let capacity = 600_00000000;
    let alice_script = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.pack())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.to_vec();
            args.extend(&[42u8; 20]);
            args.pack()
        })
        .build();
    let alice_script_hash: H256 = alice_script.hash().into();

    deposite_to_chain(
        &mut chain,
        alice_script,
        capacity,
        sudt_script_hash,
        sudt_script.clone(),
        u128::MAX,
    )
    .await
    .unwrap();

    let bob_script = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.pack())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.to_vec();
            args.extend([43u8; 20]);
            args.pack()
        })
        .build();
    let bob_script_hash: H256 = bob_script.hash().into();

    deposite_to_chain(
        &mut chain,
        bob_script.clone(),
        capacity,
        sudt_script_hash,
        sudt_script,
        u128::MAX,
    )
    .await
    .unwrap();

    let mem_pool = chain.mem_pool().as_ref().unwrap().lock().await;
    let snap = mem_pool.mem_pool_state().load();
    let tree = snap.state().unwrap();

    let alice_addr = tree
        .get_registry_address_by_script_hash(ETH_REGISTRY_ACCOUNT_ID, &alice_script_hash)
        .unwrap()
        .unwrap();
    let bob_addr = tree
        .get_registry_address_by_script_hash(ETH_REGISTRY_ACCOUNT_ID, &bob_script_hash)
        .unwrap()
        .unwrap();

    // check user account
    assert_eq!(
        tree.get_account_count().unwrap(),
        6,
        "3 builtin accounts plus 2 deposit and 1 sudt"
    );

    let alice_id = tree
        .get_account_id_by_script_hash(&alice_script_hash)
        .unwrap()
        .expect("account exists");
    assert_ne!(alice_id, 0);

    let ckb_balance = tree
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &alice_addr)
        .unwrap();
    assert_eq!(ckb_balance, CKBCapacity::from_layer1(capacity).to_layer2());

    let bob_id = tree
        .get_account_id_by_script_hash(&bob_script_hash)
        .unwrap()
        .expect("account exists");
    assert_ne!(bob_id, 0);

    let ckb_balance = tree
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &bob_addr)
        .unwrap();
    assert_eq!(ckb_balance, CKBCapacity::from_layer1(capacity).to_layer2());

    let ckb_total_supply = tree.get_sudt_total_supply(CKB_SUDT_ACCOUNT_ID).unwrap();
    assert_eq!(
        ckb_total_supply,
        CKBCapacity::from_layer1(capacity).to_layer2() * 2u8
    );

    let l2_sudt_script_hash =
        build_l2_sudt_script(chain.generator().rollup_context(), &sudt_script_hash).hash();
    let sudt_id = tree
        .get_account_id_by_script_hash(&l2_sudt_script_hash.into())
        .unwrap()
        .expect("sudt exists");

    let alice_sudt_balance = tree.get_sudt_balance(sudt_id, &alice_addr).unwrap();
    assert_eq!(alice_sudt_balance, U256::from(u128::MAX));

    let bob_sudt_balance = tree.get_sudt_balance(sudt_id, &bob_addr).unwrap();
    assert_eq!(bob_sudt_balance, U256::from(u128::MAX));

    let sudt_total_supply = tree.get_sudt_total_supply(sudt_id).unwrap();
    assert_eq!(
        sudt_total_supply,
        U256::from(u128::MAX) + U256::from(u128::MAX)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_overdraft() {
    let rollup_type_script = Script::default();
    let rollup_script_hash = rollup_type_script.hash();
    let mut chain = setup_chain(rollup_type_script.clone()).await;
    let capacity = 500_00000000;
    let user_script = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.pack())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.to_vec();
            args.extend(&[42u8; 20]);
            args.pack()
        })
        .build();
    let another_user_script = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.pack())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.to_vec();
            args.extend(&[55u8; 20]);
            args.pack()
        })
        .build();
    let user_script_hash = user_script.hash();
    // deposit
    deposite_to_chain(
        &mut chain,
        user_script,
        capacity,
        H256::zero(),
        Script::default(),
        0,
    )
    .await
    .unwrap();
    // So that we will have enough finalized custodians.
    deposite_to_chain(
        &mut chain,
        another_user_script,
        capacity,
        H256::zero(),
        Script::default(),
        0,
    )
    .await
    .unwrap();

    // wait for deposit finalize
    for _ in 0..DEFAULT_FINALITY_BLOCKS {
        produce_empty_block(&mut chain).await.unwrap();
    }

    // withdrawal
    let withdraw_capacity = 600_00000000u64;
    let err = withdrawal_from_chain(
        &mut chain,
        user_script_hash.into(),
        withdraw_capacity,
        H256::zero(),
        0,
    )
    .await
    .unwrap_err();
    assert_eq!(
        err.downcast::<gw_generator::Error>().unwrap(),
        gw_generator::Error::from(WithdrawalError::Overdraft)
    );
    // check tx pool state
    {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mem_pool = mem_pool.lock().await;
        let snap = mem_pool.mem_pool_state().load();
        let state = snap.state().unwrap();

        let user_addr = state
            .get_registry_address_by_script_hash(ETH_REGISTRY_ACCOUNT_ID, &user_script_hash.into())
            .unwrap()
            .unwrap();

        assert_eq!(
            state
                .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &user_addr)
                .unwrap(),
            CKBCapacity::from_layer1(capacity).to_layer2()
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_deposit_faked_ckb() {
    let rollup_type_script = Script::default();
    let rollup_script_hash = rollup_type_script.hash();
    let mut chain = setup_chain(rollup_type_script.clone()).await;
    let capacity = 500_00000000;
    let user_script = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.pack())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.to_vec();
            args.extend(&[42u8; 20]);
            args.pack()
        })
        .build();
    // deposit
    let err = deposite_to_chain(
        &mut chain,
        user_script,
        capacity,
        H256::zero(),
        Script::default(),
        42_00000000,
    )
    .await
    .unwrap_err();
    let err: Error = err.downcast().unwrap();
    assert_eq!(err, Error::Deposit(DepositError::DepositFakedCKB));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_produce_block_after_re_inject_withdrawal() {
    let rollup_type_script = Script::default();
    let rollup_script_hash = rollup_type_script.hash();
    let mut chain = setup_chain(rollup_type_script.clone()).await;
    let capacity = 600_00000000;
    let user_script = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.pack())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.to_vec();
            args.extend(&[42u8; 20]);
            args.pack()
        })
        .build();
    let user_script_hash = user_script.hash();
    // deposit
    deposite_to_chain(
        &mut chain,
        user_script,
        capacity,
        H256::zero(),
        Script::default(),
        0,
    )
    .await
    .unwrap();
    let (user_id, user_script_hash, user_addr, ckb_balance) = {
        let mem_pool = chain.mem_pool().as_ref().unwrap().lock().await;
        let snap = mem_pool.mem_pool_state().load();
        let tree = snap.state().unwrap();
        // check user account
        assert_eq!(
            tree.get_account_count().unwrap(),
            4,
            "3 builtin accounts plus 1 deposit"
        );
        let user_id = tree
            .get_account_id_by_script_hash(&user_script_hash.into())
            .unwrap()
            .expect("account exists");
        assert_ne!(user_id, 0);
        let user_script_hash = tree.get_script_hash(user_id).unwrap();
        let user_addr = tree
            .get_registry_address_by_script_hash(ETH_REGISTRY_ACCOUNT_ID, &user_script_hash)
            .unwrap()
            .unwrap();
        let ckb_balance = tree
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &user_addr)
            .unwrap();
        assert_eq!(ckb_balance, CKBCapacity::from_layer1(capacity).to_layer2());
        (user_id, user_script_hash, user_addr, ckb_balance)
    };

    // wait for deposit finalize
    for _ in 0..DEFAULT_FINALITY_BLOCKS {
        produce_empty_block(&mut chain).await.unwrap();
    }

    // withdrawal
    let withdraw_capacity = 322_00000000u64;
    withdrawal_from_chain(
        &mut chain,
        user_script_hash,
        withdraw_capacity,
        H256::zero(),
        0,
    )
    .await
    .unwrap();

    // Revert tip block.
    let l2block = chain.store().get_tip_block().unwrap();
    let prev_block_hash = l2block.raw().parent_block_hash().unpack();
    let prev_global_state = chain
        .store()
        .get_block_post_global_state(&prev_block_hash)
        .unwrap()
        .unwrap();
    {
        let db = chain.store().begin_transaction();
        chain
            .revert_l1action(
                &db,
                RevertedL1Action {
                    prev_global_state,
                    context: RevertL1ActionContext::SubmitValidBlock { l2block },
                },
            )
            .unwrap();
        db.commit().unwrap();
    }
    let withdrawal_hash = {
        let mem_pool = chain.mem_pool();
        let mut mem_pool = mem_pool.as_deref().unwrap().lock().await;
        mem_pool
            .notify_new_tip(prev_block_hash, &Default::default())
            .await
            .unwrap();
        mem_pool.mem_block().withdrawals()[0]
    };
    // The withdrawal should not be in db withdrawals but in db mem pool withdrawals.
    // TODO: fix this and then uncomment.
    // assert!(chain
    //     .store()
    //     .get_withdrawal(&withdrawal_hash)
    //     .unwrap()
    //     .is_none());
    assert!(chain
        .store()
        .get_mem_pool_withdrawal(&withdrawal_hash)
        .unwrap()
        .is_some());

    // Produce another block. The withdrawal should have been packaged again.
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block_with_timestamp(&chain, &mut mem_pool, Default::default(), 0, false)
            .await
            .unwrap()
    };
    let asset_scripts = HashSet::new();
    apply_block_result(&mut chain, block_result, Default::default(), asset_scripts).await;

    // check status

    let db = chain.store().begin_transaction();
    let tree = db.state_tree(StateContext::ReadOnly).unwrap();
    let ckb_balance2 = tree
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &user_addr)
        .unwrap();
    assert_eq!(
        ckb_balance,
        ckb_balance2 + CKBCapacity::from_layer1(withdraw_capacity).to_layer2()
    );
    let nonce = tree.get_nonce(user_id).unwrap();
    assert_eq!(nonce, 1);

    // The withdrawal should be in db withdrawals and not in db mem pool withdrawals.
    assert!(db.get_withdrawal(&withdrawal_hash).unwrap().is_some());
    assert!(db
        .get_mem_pool_withdrawal(&withdrawal_hash)
        .unwrap()
        .is_none());
}
