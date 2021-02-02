use anyhow::Result;
use gw_common::{builtins::CKB_SUDT_ACCOUNT_ID, state::State, H256};
use gw_generator::{
    error::{DepositionError, WithdrawalError},
    Error,
};
use gw_types::{
    packed::{CellOutput, DepositionRequest, RawWithdrawalRequest, Script, WithdrawalRequest},
    prelude::*,
};

use crate::testing_tools::{
    apply_block_result, setup_chain, ALWAYS_SUCCESS_ACCOUNT_LOCK_CODE_HASH,
};
use crate::{
    chain::{Chain, ProduceBlockParam},
    mem_pool::PackageParam,
    next_block_context::NextBlockContext,
};

fn deposite_to_chain(
    chain: &mut Chain,
    block_producer_id: u32,
    rollup_cell: CellOutput,
    user_script: Script,
    capacity: u64,
    sudt_script_hash: H256,
    amount: u128,
) -> Result<()> {
    let nb_ctx = NextBlockContext {
        block_producer_id: 0,
        timestamp: 1000,
    };
    let deposition_requests = vec![DepositionRequest::new_builder()
        .capacity(capacity.pack())
        .sudt_script_hash(sudt_script_hash.pack())
        .amount(amount.pack())
        .script(user_script)
        .build()];
    let mem_pool_package = chain.mem_pool.lock().package(PackageParam {
        deposition_requests: deposition_requests.clone(),
        max_withdrawal_capacity: std::u128::MAX,
    })?;
    let block_result =
        chain.produce_block(ProduceBlockParam { block_producer_id }, mem_pool_package)?;
    // deposit
    apply_block_result(
        chain,
        rollup_cell.clone(),
        nb_ctx.clone(),
        block_result,
        deposition_requests,
    );
    Ok(())
}

fn withdrawal_from_chain(
    chain: &mut Chain,
    block_producer_id: u32,
    rollup_cell: CellOutput,
    user_script_hash: H256,
    capacity: u64,
    sudt_script_hash: H256,
    amount: u128,
) -> Result<()> {
    let nb_ctx = NextBlockContext {
        block_producer_id: 0,
        timestamp: 1000,
    };
    let withdrawal = {
        let raw = RawWithdrawalRequest::new_builder()
            .capacity(capacity.pack())
            .account_script_hash(user_script_hash.pack())
            .sudt_script_hash(sudt_script_hash.pack())
            .amount(amount.pack())
            .build();
        WithdrawalRequest::new_builder().raw(raw).build()
    };
    let mem_pool_package = {
        let mut mem_pool = chain.mem_pool.lock();
        mem_pool.push_withdrawal_request(withdrawal)?;
        mem_pool
            .package(PackageParam {
                deposition_requests: Vec::new(),
                max_withdrawal_capacity: std::u128::MAX,
            })
            .unwrap()
    };
    let block_result = chain
        .produce_block(ProduceBlockParam { block_producer_id }, mem_pool_package)
        .unwrap();
    // deposit
    apply_block_result(
        chain,
        rollup_cell.clone(),
        nb_ctx.clone(),
        block_result,
        Vec::new(),
    );
    Ok(())
}

#[test]
fn test_deposition_and_withdrawal() {
    let rollup_type_script = Script::default();
    let mut chain = setup_chain(rollup_type_script.clone(), Default::default());
    let block_producer_id = 0;
    let capacity = 500_00000000;
    let user_script = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_ACCOUNT_LOCK_CODE_HASH.pack())
        .args(vec![42].pack())
        .build();
    let user_script_hash = user_script.hash();
    let rollup_cell = CellOutput::new_builder()
        .type_(Some(rollup_type_script).pack())
        .build();
    // deposit
    deposite_to_chain(
        &mut chain,
        block_producer_id,
        rollup_cell.clone(),
        user_script,
        capacity,
        H256::zero(),
        0,
    )
    .unwrap();
    let (user_id, ckb_balance) = {
        let db = chain.store().begin_transaction();
        let tree = db.account_state_tree().unwrap();
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
        let ckb_balance = tree.get_sudt_balance(CKB_SUDT_ACCOUNT_ID, user_id).unwrap();
        assert_eq!(ckb_balance, capacity as u128);
        (user_id, ckb_balance)
    };
    // check tx pool state
    {
        let mem_pool = chain.mem_pool.lock();
        let state = mem_pool.state();
        assert_eq!(
            state
                .get_account_id_by_script_hash(&user_script_hash.into())
                .unwrap()
                .unwrap(),
            user_id
        );
        assert_eq!(
            state
                .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, user_id)
                .unwrap(),
            capacity as u128
        );
    }
    // withdrawal
    let withdraw_capacity = 200_00000000u64;
    withdrawal_from_chain(
        &mut chain,
        block_producer_id,
        rollup_cell,
        user_script_hash.into(),
        withdraw_capacity,
        H256::zero(),
        0,
    )
    .unwrap();
    // check status
    let db = chain.store().begin_transaction();
    let tree = db.account_state_tree().unwrap();
    let ckb_balance2 = tree.get_sudt_balance(CKB_SUDT_ACCOUNT_ID, user_id).unwrap();
    assert_eq!(ckb_balance, ckb_balance2 + withdraw_capacity as u128);
    let nonce = tree.get_nonce(user_id).unwrap();
    assert_eq!(nonce, 1);
    // check tx pool state
    {
        let mem_pool = chain.mem_pool.lock();
        let state = mem_pool.state();
        assert_eq!(
            state
                .get_account_id_by_script_hash(&user_script_hash.into())
                .unwrap()
                .unwrap(),
            user_id
        );
        assert_eq!(
            state
                .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, user_id)
                .unwrap(),
            ckb_balance2
        );
        assert_eq!(state.get_nonce(user_id).unwrap(), nonce);
    }
}

#[test]
fn test_overdraft() {
    let rollup_type_script = Script::default();
    let mut chain = setup_chain(rollup_type_script.clone(), Default::default());
    let block_producer_id = 0;
    let capacity = 500_00000000;
    let user_script = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_ACCOUNT_LOCK_CODE_HASH.pack())
        .args(vec![42].pack())
        .build();
    let user_script_hash = user_script.hash();
    let rollup_cell = CellOutput::new_builder()
        .type_(Some(rollup_type_script).pack())
        .build();
    // deposit
    deposite_to_chain(
        &mut chain,
        block_producer_id,
        rollup_cell.clone(),
        user_script,
        capacity,
        H256::zero(),
        0,
    )
    .unwrap();
    // withdrawal
    let withdraw_capacity = 600_00000000u64;
    let err = withdrawal_from_chain(
        &mut chain,
        block_producer_id,
        rollup_cell,
        user_script_hash.into(),
        withdraw_capacity,
        H256::zero(),
        0,
    )
    .unwrap_err();
    let err: Error = err.downcast().unwrap();
    assert_eq!(err, Error::Withdrawal(WithdrawalError::Overdraft));
}

#[test]
fn test_deposit_faked_ckb() {
    let rollup_type_script = Script::default();
    let mut chain = setup_chain(rollup_type_script.clone(), Default::default());
    let block_producer_id = 0;
    let capacity = 500_00000000;
    let user_script = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_ACCOUNT_LOCK_CODE_HASH.pack())
        .args(vec![42].pack())
        .build();
    let rollup_cell = CellOutput::new_builder()
        .type_(Some(rollup_type_script).pack())
        .build();
    // deposit
    let err = deposite_to_chain(
        &mut chain,
        block_producer_id,
        rollup_cell.clone(),
        user_script,
        capacity,
        H256::zero(),
        42_00000000,
    )
    .unwrap_err();
    let err: Error = err.downcast().unwrap();
    assert_eq!(err, Error::Deposition(DepositionError::DepositFakedCKB));
}
