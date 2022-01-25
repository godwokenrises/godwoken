#![allow(clippy::mutable_key_type)]

use crate::testing_tool::chain::{
    apply_block_result, construct_block, setup_chain, ALWAYS_SUCCESS_CODE_HASH,
    DEFAULT_FINALITY_BLOCKS,
};

use anyhow::Result;
use gw_chain::chain::Chain;
use gw_common::{
    builtins::CKB_SUDT_ACCOUNT_ID,
    state::{to_short_script_hash, State},
    H256, U256,
};
use gw_generator::{
    error::{DepositError, WithdrawalError},
    sudt::build_l2_sudt_script,
    Error,
};
use gw_store::state::state_db::StateContext;
use gw_types::{
    core::ScriptHashType,
    packed::{CellOutput, DepositRequest, RawWithdrawalRequest, Script, WithdrawalRequest},
    prelude::*,
};

use std::{collections::HashSet, iter::FromIterator};

async fn produce_empty_block(chain: &mut Chain, rollup_cell: CellOutput) -> Result<()> {
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(chain, &mut mem_pool, Default::default()).await?
    };
    let asset_scripts = HashSet::new();

    // deposit
    apply_block_result(chain, rollup_cell, block_result, vec![], asset_scripts).await;
    Ok(())
}

async fn deposite_to_chain(
    chain: &mut Chain,
    rollup_cell: CellOutput,
    user_script: Script,
    capacity: u64,
    sudt_script_hash: H256, // To allow deposit ckb only
    sudt_script: Script,
    amount: u128,
) -> Result<()> {
    let deposit_requests = vec![DepositRequest::new_builder()
        .capacity(capacity.pack())
        .sudt_script_hash(sudt_script_hash.pack())
        .amount(amount.pack())
        .script(user_script)
        .build()];
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(chain, &mut mem_pool, deposit_requests.clone()).await?
    };
    let asset_scripts = if sudt_script_hash == H256::zero() {
        HashSet::new()
    } else {
        HashSet::from_iter(vec![sudt_script])
    };

    // deposit
    apply_block_result(
        chain,
        rollup_cell,
        block_result,
        deposit_requests,
        asset_scripts,
    )
    .await;
    Ok(())
}

async fn withdrawal_from_chain(
    chain: &mut Chain,
    rollup_cell: CellOutput,
    user_script_hash: H256,
    capacity: u64,
    sudt_script_hash: H256,
    amount: u128,
) -> Result<()> {
    let withdrawal = {
        let raw = RawWithdrawalRequest::new_builder()
            .capacity(capacity.pack())
            .account_script_hash(user_script_hash.pack())
            .sudt_script_hash(sudt_script_hash.pack())
            .amount(amount.pack())
            .build();
        WithdrawalRequest::new_builder().raw(raw).build()
    };
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        mem_pool.push_withdrawal_request(withdrawal.into()).await?;
        construct_block(chain, &mut mem_pool, Vec::default())
            .await
            .unwrap()
    };

    // deposit
    apply_block_result(chain, rollup_cell, block_result, Vec::new(), HashSet::new()).await;
    Ok(())
}

#[tokio::test]
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
            args.push(42);
            args.pack()
        })
        .build();
    let user_script_hash = user_script.hash();
    let rollup_cell = CellOutput::new_builder()
        .type_(Some(rollup_type_script).pack())
        .build();
    // deposit
    deposite_to_chain(
        &mut chain,
        rollup_cell.clone(),
        user_script,
        capacity,
        H256::zero(),
        Script::default(),
        0,
    )
    .await
    .unwrap();
    let (user_id, user_script_hash, ckb_balance, ckb_total_supply) = {
        let mem_pool = chain.mem_pool().as_ref().unwrap().lock().await;
        let snap = mem_pool.mem_pool_state().load();
        let tree = snap.state().unwrap();
        // check user account
        assert_eq!(
            tree.get_account_count().unwrap(),
            3,
            "2 builtin accounts plus 1 deposit"
        );
        let user_id = tree
            .get_account_id_by_script_hash(&user_script_hash.into())
            .unwrap()
            .expect("account exists");
        assert_ne!(user_id, 0);
        let user_script_hash = tree.get_script_hash(user_id).unwrap();
        let ckb_balance = tree
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, to_short_script_hash(&user_script_hash))
            .unwrap();
        assert_eq!(ckb_balance, capacity as u128);
        let ckb_total_supply = tree.get_sudt_total_supply(CKB_SUDT_ACCOUNT_ID).unwrap();
        assert_eq!(ckb_total_supply, U256::from(capacity));
        (user_id, user_script_hash, ckb_balance, ckb_total_supply)
    };

    // wait for deposit finalize
    for _ in 0..DEFAULT_FINALITY_BLOCKS {
        produce_empty_block(&mut chain, rollup_cell.clone())
            .await
            .unwrap();
    }
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
                .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, to_short_script_hash(&user_script_hash))
                .unwrap(),
            capacity as u128
        );
        assert_eq!(
            state.get_sudt_total_supply(CKB_SUDT_ACCOUNT_ID).unwrap(),
            U256::from(capacity)
        )
    }
    // withdrawal
    let withdraw_capacity = 300_00000000u64;
    withdrawal_from_chain(
        &mut chain,
        rollup_cell,
        user_script_hash,
        withdraw_capacity,
        H256::zero(),
        0,
    )
    .await
    .unwrap();
    // check status
    let db = chain.store().begin_transaction();
    let tree = db.state_tree(StateContext::ReadOnly).unwrap();
    let ckb_balance2 = tree
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, to_short_script_hash(&user_script_hash))
        .unwrap();
    assert_eq!(ckb_balance, ckb_balance2 + withdraw_capacity as u128);
    let ckb_total_supply2 = tree.get_sudt_total_supply(CKB_SUDT_ACCOUNT_ID).unwrap();
    assert_eq!(
        ckb_total_supply,
        ckb_total_supply2 + U256::from(withdraw_capacity)
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
                .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, to_short_script_hash(&user_script_hash))
                .unwrap(),
            ckb_balance2
        );
        assert_eq!(
            state.get_sudt_total_supply(CKB_SUDT_ACCOUNT_ID).unwrap(),
            U256::from(ckb_balance2)
        );
        assert_eq!(state.get_nonce(user_id).unwrap(), nonce);
    }
}

#[tokio::test]
async fn test_deposit_u128_overflow() {
    let rollup_type_script = Script::default();
    let rollup_script_hash = rollup_type_script.hash();
    let mut chain = setup_chain(rollup_type_script.clone()).await;
    let rollup_cell = CellOutput::new_builder()
        .type_(Some(rollup_type_script).pack())
        .build();

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
            args.push(42);
            args.pack()
        })
        .build();
    let alice_script_hash: H256 = alice_script.hash().into();

    deposite_to_chain(
        &mut chain,
        rollup_cell.clone(),
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
            args.push(43);
            args.pack()
        })
        .build();
    let bob_script_hash: H256 = bob_script.hash().into();

    deposite_to_chain(
        &mut chain,
        rollup_cell.clone(),
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

    // check user account
    assert_eq!(
        tree.get_account_count().unwrap(),
        5,
        "2 builtin accounts plus 2 deposit and 1 sudt"
    );

    let alice_id = tree
        .get_account_id_by_script_hash(&alice_script_hash)
        .unwrap()
        .expect("account exists");
    assert_ne!(alice_id, 0);

    let alice_script_hash = tree.get_script_hash(alice_id).unwrap();
    let ckb_balance = tree
        .get_sudt_balance(
            CKB_SUDT_ACCOUNT_ID,
            to_short_script_hash(&alice_script_hash),
        )
        .unwrap();
    assert_eq!(ckb_balance, capacity as u128);

    let bob_id = tree
        .get_account_id_by_script_hash(&bob_script_hash)
        .unwrap()
        .expect("account exists");
    assert_ne!(bob_id, 0);

    let bob_script_hash = tree.get_script_hash(bob_id).unwrap();
    let ckb_balance = tree
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, to_short_script_hash(&bob_script_hash))
        .unwrap();
    assert_eq!(ckb_balance, capacity as u128);

    let ckb_total_supply = tree.get_sudt_total_supply(CKB_SUDT_ACCOUNT_ID).unwrap();
    assert_eq!(ckb_total_supply, U256::from(capacity * 2));

    let l2_sudt_script_hash =
        build_l2_sudt_script(chain.generator().rollup_context(), &sudt_script_hash).hash();
    let sudt_id = tree
        .get_account_id_by_script_hash(&l2_sudt_script_hash.into())
        .unwrap()
        .expect("sudt exists");

    let alice_sudt_balance = tree
        .get_sudt_balance(sudt_id, to_short_script_hash(&alice_script_hash))
        .unwrap();
    assert_eq!(alice_sudt_balance, u128::MAX);

    let bob_sudt_balance = tree
        .get_sudt_balance(sudt_id, to_short_script_hash(&bob_script_hash))
        .unwrap();
    assert_eq!(bob_sudt_balance, u128::MAX);

    let sudt_total_supply = tree.get_sudt_total_supply(sudt_id).unwrap();
    assert_eq!(
        sudt_total_supply,
        U256::from(u128::MAX) + U256::from(u128::MAX)
    );
}

#[tokio::test]
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
            args.push(42);
            args.pack()
        })
        .build();
    let user_script_hash = user_script.hash();
    let rollup_cell = CellOutput::new_builder()
        .type_(Some(rollup_type_script).pack())
        .build();
    // deposit
    deposite_to_chain(
        &mut chain,
        rollup_cell.clone(),
        user_script,
        capacity,
        H256::zero(),
        Script::default(),
        0,
    )
    .await
    .unwrap();

    // withdrawal
    let withdraw_capacity = 600_00000000u64;
    let err = withdrawal_from_chain(
        &mut chain,
        rollup_cell,
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
        assert_eq!(
            state
                .get_sudt_balance(
                    CKB_SUDT_ACCOUNT_ID,
                    to_short_script_hash(&user_script_hash.into())
                )
                .unwrap(),
            capacity as u128
        );
    }
}

#[tokio::test]
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
            args.push(42);
            args.pack()
        })
        .build();
    let rollup_cell = CellOutput::new_builder()
        .type_(Some(rollup_type_script).pack())
        .build();
    // deposit
    let err = deposite_to_chain(
        &mut chain,
        rollup_cell,
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
