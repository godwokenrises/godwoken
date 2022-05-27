use std::time::Duration;

use ckb_types::prelude::{Builder, Entity};
use gw_common::{
    builtins::{CKB_SUDT_ACCOUNT_ID, ETH_REGISTRY_ACCOUNT_ID},
    state::State,
    H256,
};
use gw_types::{
    packed::{RawL2Transaction, Script},
    prelude::{Pack, Unpack},
    U256,
};

use crate::testing_tool::{
    chain::setup_chain,
    eth_wallet::EthWallet,
    polyjuice::{erc20::SudtErc20ArgsBuilder, PolyjuiceAccount, PolyjuiceSystemLog},
    rpc_server::{wait_tx_committed, RPCServer},
};

#[tokio::test(flavor = "multi_thread")]
async fn test_polyjuice_erc20_tx() {
    let _ = env_logger::builder().is_test(true).try_init();

    let rollup_type_script = Script::default();
    let rollup_script_hash: H256 = rollup_type_script.hash().into();

    let chain = setup_chain(rollup_type_script.clone()).await;
    let chain_id = {
        let config = &chain.generator().rollup_context().rollup_config;
        config.chain_id().unpack()
    };
    let rpc_server = RPCServer::build(&chain, rollup_type_script, None)
        .await
        .unwrap();

    let mem_pool_state = {
        let mem_pool = chain.mem_pool().as_ref().unwrap().lock().await;
        mem_pool.mem_pool_state()
    };
    let snap = mem_pool_state.load();
    let mut state = snap.state().unwrap();

    let creator_wallet = EthWallet::random(rollup_script_hash);
    let creator_account_id = creator_wallet
        .create_account(&mut state, 1000000u128.into())
        .unwrap();

    let polyjuice_account = PolyjuiceAccount::create(rollup_script_hash, &mut state).unwrap();
    state.submit_tree_to_mem_block();

    let deploy_args = SudtErc20ArgsBuilder::deploy(CKB_SUDT_ACCOUNT_ID, 18).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain_id.pack())
        .from_id(creator_account_id.pack())
        .to_id(polyjuice_account.id.pack())
        .nonce(0u32.pack())
        .args(deploy_args.pack())
        .build();
    let erc20_deploy_tx = creator_wallet.sign_polyjuice_tx(&state, raw_tx).unwrap();
    let erc20_deploy_tx_hash = rpc_server
        .submit_l2transaction(&erc20_deploy_tx)
        .await
        .unwrap();

    wait_tx_committed(&chain, &erc20_deploy_tx_hash, Duration::from_secs(30))
        .await
        .unwrap();

    let system_log = PolyjuiceSystemLog::parse_from_tx_hash(&chain, erc20_deploy_tx_hash).unwrap();
    assert_eq!(system_log.status_code, 0);

    let erc20_contract_account_id = system_log.contract_account_id(&state).unwrap();
    let to_wallet = EthWallet::random(rollup_script_hash);
    let transfer_args =
        SudtErc20ArgsBuilder::transfer(&to_wallet.registry_address, U256::from(100u128)).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain_id.pack())
        .from_id(creator_account_id.pack())
        .to_id(erc20_contract_account_id.pack())
        .nonce(1u32.pack())
        .args(transfer_args.pack())
        .build();

    let erc20_transfer_tx = creator_wallet.sign_polyjuice_tx(&state, raw_tx).unwrap();
    let erc20_transfer_tx_hash = rpc_server
        .submit_l2transaction(&erc20_transfer_tx)
        .await
        .unwrap();

    wait_tx_committed(&chain, &erc20_transfer_tx_hash, Duration::from_secs(30))
        .await
        .unwrap();

    let system_log = PolyjuiceSystemLog::parse_from_tx_hash(&chain, erc20_deploy_tx_hash).unwrap();
    assert_eq!(system_log.status_code, 0);

    let balance = state
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &to_wallet.registry_address)
        .unwrap();
    assert_eq!(balance, U256::from(100u128));
}

#[tokio::test(flavor = "multi_thread")]
async fn test_polyjuice_tx_from_id_zero() {
    let _ = env_logger::builder().is_test(true).try_init();

    let rollup_type_script = Script::default();
    let rollup_script_hash: H256 = rollup_type_script.hash().into();

    let chain = setup_chain(rollup_type_script.clone()).await;
    let chain_id = {
        let config = &chain.generator().rollup_context().rollup_config;
        config.chain_id().unpack()
    };

    let mem_pool_state = {
        let mem_pool = chain.mem_pool().as_ref().unwrap().lock().await;
        mem_pool.mem_pool_state()
    };
    let snap = mem_pool_state.load();
    let mut state = snap.state().unwrap();

    let creator_wallet = EthWallet::random(rollup_script_hash);
    creator_wallet
        .create_account(&mut state, 1000000u128.into())
        .unwrap();

    let rpc_server = RPCServer::build(&chain, rollup_type_script, Some(creator_wallet.inner))
        .await
        .unwrap();

    let polyjuice_account = PolyjuiceAccount::create(rollup_script_hash, &mut state).unwrap();

    let test_wallet = EthWallet::random(rollup_script_hash);
    let balance: U256 = 1000000u128.into();
    state
        .mint_sudt(CKB_SUDT_ACCOUNT_ID, &test_wallet.registry_address, balance)
        .unwrap();
    state.submit_tree_to_mem_block();

    let deploy_args = SudtErc20ArgsBuilder::deploy(CKB_SUDT_ACCOUNT_ID, 18).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain_id.pack())
        .from_id(0u32.pack())
        .to_id(polyjuice_account.id.pack())
        .nonce(0u32.pack())
        .args(deploy_args.pack())
        .build();
    let erc20_deploy_tx = test_wallet.sign_polyjuice_tx(&state, raw_tx).unwrap();
    let erc20_deploy_tx_hash = rpc_server
        .submit_l2transaction(&erc20_deploy_tx)
        .await
        .unwrap();

    wait_tx_committed(&chain, &erc20_deploy_tx_hash, Duration::from_secs(30))
        .await
        .unwrap();

    let system_log = PolyjuiceSystemLog::parse_from_tx_hash(&chain, erc20_deploy_tx_hash).unwrap();
    assert_eq!(system_log.status_code, 0);

    let test_account_id = state
        .get_account_id_by_script_hash(&test_wallet.account_script().hash().into())
        .unwrap();
    assert!(test_account_id.is_some());

    let test_registry_address = state
        .get_registry_address_by_script_hash(
            ETH_REGISTRY_ACCOUNT_ID,
            &test_wallet.account_script().hash().into(),
        )
        .unwrap();
    assert_eq!(
        test_registry_address.as_ref(),
        Some(&test_wallet.registry_address)
    );

    let balance_after = state
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &test_wallet.registry_address)
        .unwrap();

    assert!(balance > balance_after);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_polyjuice_tx_from_id_zero_insufficient_balance() {
    let _ = env_logger::builder().is_test(true).try_init();

    let rollup_type_script = Script::default();
    let rollup_script_hash: H256 = rollup_type_script.hash().into();

    let chain = setup_chain(rollup_type_script.clone()).await;
    let chain_id = {
        let config = &chain.generator().rollup_context().rollup_config;
        config.chain_id().unpack()
    };

    let mem_pool_state = {
        let mem_pool = chain.mem_pool().as_ref().unwrap().lock().await;
        mem_pool.mem_pool_state()
    };
    let snap = mem_pool_state.load();
    let mut state = snap.state().unwrap();

    let creator_wallet = EthWallet::random(rollup_script_hash);
    creator_wallet
        .create_account(&mut state, 1000000u128.into())
        .unwrap();

    let test_wallet = EthWallet::random(rollup_script_hash);
    let rpc_server = RPCServer::build(&chain, rollup_type_script, Some(creator_wallet.inner))
        .await
        .unwrap();

    let polyjuice_account = PolyjuiceAccount::create(rollup_script_hash, &mut state).unwrap();
    state.submit_tree_to_mem_block();

    let deploy_args = SudtErc20ArgsBuilder::deploy(CKB_SUDT_ACCOUNT_ID, 18).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain_id.pack())
        .from_id(0u32.pack())
        .to_id(polyjuice_account.id.pack())
        .nonce(0u32.pack())
        .args(deploy_args.pack())
        .build();
    let erc20_deploy_tx = test_wallet.sign_polyjuice_tx(&state, raw_tx).unwrap();

    let err = rpc_server
        .submit_l2transaction(&erc20_deploy_tx)
        .await
        .unwrap_err();
    eprintln!("err {}", err);

    assert!(err.to_string().contains("unregistered EOA account"))
}

#[tokio::test(flavor = "multi_thread")]
async fn test_polyjuice_tx_from_id_zero_with_registered_registry_address() {
    let _ = env_logger::builder().is_test(true).try_init();

    let rollup_type_script = Script::default();
    let rollup_script_hash: H256 = rollup_type_script.hash().into();

    let chain = setup_chain(rollup_type_script.clone()).await;
    let chain_id = {
        let config = &chain.generator().rollup_context().rollup_config;
        config.chain_id().unpack()
    };

    let mem_pool_state = {
        let mem_pool = chain.mem_pool().as_ref().unwrap().lock().await;
        mem_pool.mem_pool_state()
    };
    let snap = mem_pool_state.load();
    let mut state = snap.state().unwrap();

    let creator_wallet = EthWallet::random(rollup_script_hash);
    creator_wallet
        .create_account(&mut state, 1000000u128.into())
        .unwrap();

    let test_wallet = EthWallet::random(rollup_script_hash);
    let test_script_hash: H256 = Script::default().hash().into();
    state
        .mapping_registry_address_to_script_hash(
            test_wallet.registry_address.to_owned(),
            test_script_hash,
        )
        .unwrap();

    let rpc_server = RPCServer::build(&chain, rollup_type_script, Some(creator_wallet.inner))
        .await
        .unwrap();

    let polyjuice_account = PolyjuiceAccount::create(rollup_script_hash, &mut state).unwrap();
    state.submit_tree_to_mem_block();

    let deploy_args = SudtErc20ArgsBuilder::deploy(CKB_SUDT_ACCOUNT_ID, 18).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain_id.pack())
        .from_id(0u32.pack())
        .to_id(polyjuice_account.id.pack())
        .nonce(0u32.pack())
        .args(deploy_args.pack())
        .build();
    let erc20_deploy_tx = test_wallet.sign_polyjuice_tx(&state, raw_tx).unwrap();

    let err = rpc_server
        .submit_l2transaction(&erc20_deploy_tx)
        .await
        .unwrap_err();
    eprintln!("err {}", err);

    assert!(err.to_string().contains("unregistered EOA account"))
}
