use std::time::Duration;

use gw_common::{
    builtins::{CKB_SUDT_ACCOUNT_ID, ETH_REGISTRY_ACCOUNT_ID},
    ckb_decimal::CKBCapacity,
    state::State,
};
use gw_generator::account_lock_manage::eip712::{self, traits::EIP712Encode};
use gw_types::{
    h256::*,
    packed::{
        DepositInfoVec, DepositRequest, RawWithdrawalRequest, Script, WithdrawalRequest,
        WithdrawalRequestExtra,
    },
    prelude::*,
};

use crate::testing_tool::{
    chain::{into_deposit_info_cell, produce_empty_block, TestChain, DEFAULT_FINALITY_BLOCKS},
    eth_wallet::EthWallet,
    rpc_server::RPCServer,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_submit_withdrawal_request() {
    let _ = env_logger::builder().is_test(true).try_init();

    let rollup_type_script = Script::default();
    let mut chain = TestChain::setup(rollup_type_script).await;
    let rpc_server = RPCServer::build(&chain, None).await.unwrap();

    // Deposit test account
    const DEPOSIT_CAPACITY: u64 = 12345768 * 10u64.pow(8);
    let test_wallet = EthWallet::random(chain.rollup_type_hash());
    let deposit = DepositRequest::new_builder()
        .capacity(DEPOSIT_CAPACITY.pack())
        .sudt_script_hash(H256::zero().pack())
        .amount(0.pack())
        .script(test_wallet.account_script().to_owned())
        .registry_id(ETH_REGISTRY_ACCOUNT_ID.pack())
        .build();
    let deposit_info_vec = DepositInfoVec::new_builder()
        .push(into_deposit_info_cell(chain.inner.generator().rollup_context(), deposit).pack())
        .build();
    chain.produce_block(deposit_info_vec, vec![]).await.unwrap();

    for _ in 0..DEFAULT_FINALITY_BLOCKS + 1 {
        produce_empty_block(&mut chain.inner).await.unwrap();
    }

    let mem_pool_state = chain.mem_pool_state().await;
    let state = mem_pool_state.load_state_db();

    let balance_before_withdrawal = state
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, test_wallet.reg_address())
        .unwrap();

    const WITHDRAWAL_CAPACITY: u64 = 1000u64 * 10u64.pow(8);
    let withdrawal = {
        let raw = RawWithdrawalRequest::new_builder()
            .chain_id(chain.chain_id().pack())
            .capacity(WITHDRAWAL_CAPACITY.pack())
            .amount(0.pack())
            .account_script_hash(test_wallet.account_script_hash().pack())
            .owner_lock_hash(test_wallet.account_script_hash().pack())
            .registry_id(gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID.pack())
            .build();
        let typed_withdrawal = eip712::types::Withdrawal::from_raw(
            raw.clone(),
            test_wallet.account_script().to_owned(),
            test_wallet.registry_address.clone(),
        )
        .unwrap();
        let domain_seperator = eip712::types::EIP712Domain {
            name: "Godwoken".to_string(),
            version: "1".to_string(),
            chain_id: chain.chain_id(),
            verifying_contract: None,
            salt: None,
        };
        let message = typed_withdrawal.eip712_message(domain_seperator.hash_struct());
        let sig = test_wallet.sign_message(message).unwrap();
        let req = WithdrawalRequest::new_builder()
            .raw(raw)
            .signature(sig.pack())
            .build();
        WithdrawalRequestExtra::new_builder()
            .request(req)
            .owner_lock(test_wallet.account_script().to_owned())
            .build()
    };

    let withdrawal_hash = rpc_server
        .submit_withdrawal_request(&withdrawal)
        .await
        .unwrap();

    while rpc_server
        .is_request_in_queue(withdrawal_hash)
        .await
        .unwrap()
    {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    chain
        .produce_block(Default::default(), vec![withdrawal])
        .await
        .unwrap();

    let state = mem_pool_state.load_state_db();
    let balance_after_withdrawal = state
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, test_wallet.reg_address())
        .unwrap();

    assert_eq!(
        balance_before_withdrawal,
        balance_after_withdrawal + CKBCapacity::from_layer1(WITHDRAWAL_CAPACITY).to_layer2()
    );
}
