#![allow(clippy::mutable_key_type)]

use crate::testing_tool::{
    chain::{into_deposit_info_cell, TestChain},
    eth_wallet::EthWallet,
    mem_pool_provider::DummyMemPoolProvider,
    polyjuice::{erc20::SudtErc20ArgsBuilder, PolyjuiceAccount, PolyjuiceSystemLog},
    rpc_server::{wait_tx_committed, RPCServer},
};

use gw_block_producer::produce_block::generate_produce_block_param;
use gw_common::{
    builtins::{CKB_SUDT_ACCOUNT_ID, ETH_REGISTRY_ACCOUNT_ID, RESERVED_ACCOUNT_ID},
    state::State,
    H256,
};
use gw_generator::account_lock_manage::secp256k1::Secp256k1Eth;
use gw_mem_pool::pool::OutputParam;
use gw_types::{
    packed::{
        CreateAccount, DepositInfoVec, DepositRequest, Fee, L2Transaction, MetaContractArgs,
        RawL2Transaction, Script,
    },
    prelude::*,
    U256,
};

use std::time::Duration;

const META_CONTRACT_ACCOUNT_ID: u32 = RESERVED_ACCOUNT_ID;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_refresh_deposit_when_mem_block_contains_tx_from_pending_create_sender() {
    let _ = env_logger::builder().is_test(true).try_init();

    const DEPOSIT_CAPACITY: u64 = 1000_00000000;

    let rollup_type_script = Script::default();
    let mut chain = TestChain::setup(rollup_type_script).await;

    // Deposit test account
    let alice = EthWallet::random(chain.rollup_type_hash());
    let deposit = DepositRequest::new_builder()
        .capacity(DEPOSIT_CAPACITY.pack())
        .sudt_script_hash(H256::zero().pack())
        .amount(0.pack())
        .script(alice.account_script().to_owned())
        .registry_id(ETH_REGISTRY_ACCOUNT_ID.pack())
        .build();
    let deposit_info_vec = DepositInfoVec::new_builder()
        .push(into_deposit_info_cell(chain.inner.generator().rollup_context(), deposit).pack())
        .build();
    chain.produce_block(deposit_info_vec, vec![]).await.unwrap();

    let mem_pool_state = chain.mem_pool_state().await;
    let snap = mem_pool_state.load();
    let state = snap.state().unwrap();

    let alice_id = state
        .get_account_id_by_script_hash(&alice.account_script_hash())
        .unwrap()
        .unwrap();

    // Deploy polyjuice
    let polyjuice_account = PolyjuiceAccount::build_script(chain.rollup_type_hash());
    let meta_contract_script_hash = state.get_script_hash(META_CONTRACT_ACCOUNT_ID).unwrap();
    let fee = Fee::new_builder()
        .registry_id(ETH_REGISTRY_ACCOUNT_ID.pack())
        .amount(0u128.pack())
        .build();
    let create_polyjuice = CreateAccount::new_builder()
        .fee(fee)
        .script(polyjuice_account.clone())
        .build();
    let args = MetaContractArgs::new_builder()
        .set(create_polyjuice)
        .build();

    let raw_l2tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(alice_id.pack())
        .to_id(META_CONTRACT_ACCOUNT_ID.pack())
        .nonce(0u32.pack())
        .args(args.as_bytes().pack())
        .build();

    let signing_message = Secp256k1Eth::eip712_signing_message(
        chain.chain_id(),
        &raw_l2tx,
        alice.reg_address().to_owned(),
        meta_contract_script_hash,
    )
    .unwrap();
    let sign = alice.sign_message(signing_message.into()).unwrap();

    let deploy_tx = L2Transaction::new_builder()
        .raw(raw_l2tx)
        .signature(sign.pack())
        .build();
    {
        let mut mem_pool = chain.mem_pool().await;
        mem_pool.push_transaction(deploy_tx).unwrap();
    }

    let snap = mem_pool_state.load();
    let state = snap.state().unwrap();

    // Depoly erc20 contract
    let polyjuice_account_id = state
        .get_account_id_by_script_hash(&polyjuice_account.hash().into())
        .unwrap()
        .unwrap();

    let deploy_args = SudtErc20ArgsBuilder::deploy(CKB_SUDT_ACCOUNT_ID, 18).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(alice_id.pack())
        .to_id(polyjuice_account_id.pack())
        .nonce(1u32.pack())
        .args(deploy_args.pack())
        .build();

    let deploy_tx = alice.sign_polyjuice_tx(&state, raw_tx).unwrap();
    let deploy_tx_hash: H256 = deploy_tx.hash().into();

    {
        let mut mem_pool = chain.mem_pool().await;
        mem_pool.push_transaction(deploy_tx).unwrap();
    }

    let snap = mem_pool_state.load();
    let state = snap.state().unwrap();

    let system_log = PolyjuiceSystemLog::parse_from_tx_hash(&chain, deploy_tx_hash).unwrap();
    assert_eq!(system_log.status_code, 0);

    // Transfer unregistered bob some token
    let bob = EthWallet::random(chain.rollup_type_hash());
    let transfer_amount: U256 = 40000u128.into();

    let erc20_contract_account_id = system_log.contract_account_id(&state).unwrap();
    let transfer_args = SudtErc20ArgsBuilder::transfer(bob.reg_address(), transfer_amount).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(alice_id.pack())
        .to_id(erc20_contract_account_id.pack())
        .nonce(2u32.pack())
        .args(transfer_args.pack())
        .build();

    let transfer_tx = alice.sign_polyjuice_tx(&state, raw_tx).unwrap();

    {
        let mut mem_pool = chain.mem_pool().await;
        mem_pool.push_transaction(transfer_tx).unwrap();
    }
    chain
        .produce_block(Default::default(), vec![])
        .await
        .unwrap();

    // Transfer from id zero
    let snap = mem_pool_state.load();
    let state = snap.state().unwrap();

    let bob_balance = state
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, bob.reg_address())
        .unwrap();
    assert_eq!(bob_balance, transfer_amount);

    let ciri = EthWallet::random(chain.rollup_type_hash());
    let transfer_args = SudtErc20ArgsBuilder::transfer(ciri.reg_address(), 100u32.into()).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(0u32.pack())
        .to_id(erc20_contract_account_id.pack())
        .nonce(0u32.pack())
        .args(transfer_args.pack())
        .build();

    let transfer_tx_hash = {
        let id = state.get_account_count().unwrap();
        let raw = raw_tx.clone().as_builder().from_id(id.pack()).build();
        raw.hash().into()
    };
    let transfer_tx = bob.sign_polyjuice_tx(&state, raw_tx).unwrap();

    let rpc_server = RPCServer::build(&chain, Some(alice.inner)).await.unwrap();
    rpc_server.submit_l2transaction(&transfer_tx).await.unwrap();
    wait_tx_committed(&chain, &transfer_tx_hash, Duration::from_secs(30))
        .await
        .unwrap();

    let snap = mem_pool_state.load();
    let state = snap.state().unwrap();

    let ciri_balance = state
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, ciri.reg_address())
        .unwrap();
    assert_eq!(ciri_balance, 100u32.into());

    // Create pending deposit for refresh
    let triss = EthWallet::random(chain.rollup_type_hash());
    let deposit = DepositRequest::new_builder()
        .capacity(DEPOSIT_CAPACITY.pack())
        .sudt_script_hash(H256::zero().pack())
        .amount(0.pack())
        .script(triss.account_script().to_owned())
        .registry_id(ETH_REGISTRY_ACCOUNT_ID.pack())
        .build();
    let deposit_info = into_deposit_info_cell(chain.inner.generator().rollup_context(), deposit);

    let mut mem_pool = chain.mem_pool().await;
    let provider = DummyMemPoolProvider {
        deposit_cells: vec![deposit_info],
        fake_blocktime: Duration::from_millis(0),
    };
    mem_pool.set_provider(Box::new(provider));
    mem_pool.reset_mem_block(&Default::default()).await.unwrap();

    let (mem_block, post_merkle_state) = mem_pool.output_mem_block(&OutputParam::default());
    let block_param =
        generate_produce_block_param(chain.store(), mem_block, post_merkle_state).unwrap();

    assert!(block_param.deposits.is_empty());
    assert_eq!(block_param.txs.len(), 2);

    let snap = mem_pool_state.load();
    let state = snap.state().unwrap();

    let ciri_balance = state
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, ciri.reg_address())
        .unwrap();
    assert_eq!(ciri_balance, 100u32.into());
}
