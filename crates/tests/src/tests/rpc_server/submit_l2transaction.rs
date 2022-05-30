use std::time::Duration;

use anyhow::anyhow;
use ckb_types::prelude::{Builder, Entity};
use gw_common::{
    builtins::{CKB_SUDT_ACCOUNT_ID, ETH_REGISTRY_ACCOUNT_ID},
    h256_ext::H256Ext,
    state::State,
    H256,
};
use gw_rpc_server::polyjuice_tx::{
    error::PolyjuiceTxSenderRecoverError, eth_context::MIN_RECOVER_CKB_BALANCE,
};
use gw_types::{
    packed::{RawL2Transaction, Script},
    prelude::Pack,
    U256,
};

use crate::testing_tool::{
    chain::TestChain,
    eth_wallet::EthWallet,
    polyjuice::{erc20::SudtErc20ArgsBuilder, PolyjuiceAccount, PolyjuiceSystemLog},
    rpc_server::{wait_tx_committed, RPCServer},
};

#[tokio::test(flavor = "multi_thread")]
async fn test_polyjuice_erc20_tx() {
    let _ = env_logger::builder().is_test(true).try_init();

    let rollup_type_script = Script::default();
    let chain = TestChain::setup(rollup_type_script).await;
    let rpc_server = RPCServer::build(&chain, None).await.unwrap();

    let mem_pool_state = chain.mem_pool_state().await;
    let snap = mem_pool_state.load();
    let mut state = snap.state().unwrap();

    let creator_wallet = EthWallet::random(chain.rollup_type_hash());
    let creator_account_id = creator_wallet
        .create_account(&mut state, 1000000u128.into())
        .unwrap();

    let polyjuice_account = PolyjuiceAccount::create(chain.rollup_type_hash(), &mut state).unwrap();
    state.submit_tree_to_mem_block();

    let deploy_args = SudtErc20ArgsBuilder::deploy(CKB_SUDT_ACCOUNT_ID, 18).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(creator_account_id.pack())
        .to_id(polyjuice_account.id.pack())
        .nonce(0u32.pack())
        .args(deploy_args.pack())
        .build();

    let deploy_tx = creator_wallet.sign_polyjuice_tx(&state, raw_tx).unwrap();
    let deploy_tx_hash = rpc_server.submit_l2transaction(&deploy_tx).await.unwrap();
    wait_tx_committed(&chain, &deploy_tx_hash, Duration::from_secs(30))
        .await
        .unwrap();

    let system_log = PolyjuiceSystemLog::parse_from_tx_hash(&chain, deploy_tx_hash).unwrap();
    assert_eq!(system_log.status_code, 0);

    let erc20_contract_account_id = system_log.contract_account_id(&state).unwrap();
    let to_wallet = EthWallet::random(chain.rollup_type_hash());
    let amount: U256 = 100u128.into();

    let transfer_args = SudtErc20ArgsBuilder::transfer(to_wallet.reg_address(), amount).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(creator_account_id.pack())
        .to_id(erc20_contract_account_id.pack())
        .nonce(1u32.pack())
        .args(transfer_args.pack())
        .build();

    let transfer_tx = creator_wallet.sign_polyjuice_tx(&state, raw_tx).unwrap();
    let transfer_tx_hash = rpc_server.submit_l2transaction(&transfer_tx).await.unwrap();
    wait_tx_committed(&chain, &transfer_tx_hash, Duration::from_secs(30))
        .await
        .unwrap();

    let system_log = PolyjuiceSystemLog::parse_from_tx_hash(&chain, deploy_tx_hash).unwrap();
    assert_eq!(system_log.status_code, 0);

    let balance = state
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, to_wallet.reg_address())
        .unwrap();
    assert_eq!(balance, amount);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_polyjuice_tx_from_id_zero() {
    let _ = env_logger::builder().is_test(true).try_init();

    let rollup_type_script = Script::default();
    let chain = TestChain::setup(rollup_type_script).await;

    let mem_pool_state = chain.mem_pool_state().await;
    let snap = mem_pool_state.load();
    let mut state = snap.state().unwrap();

    let creator_wallet = EthWallet::random(chain.rollup_type_hash());
    creator_wallet
        .create_account(&mut state, 1000000u128.into())
        .unwrap();
    let rpc_server = RPCServer::build(&chain, Some(creator_wallet.inner))
        .await
        .unwrap();

    let polyjuice_account = PolyjuiceAccount::create(chain.rollup_type_hash(), &mut state).unwrap();

    let test_wallet = EthWallet::random(chain.rollup_type_hash());
    let balance: U256 = 1000000u128.into();
    test_wallet.mint_ckb_sudt(&mut state, balance).unwrap();
    state.submit_tree_to_mem_block();

    let deploy_args = SudtErc20ArgsBuilder::deploy(CKB_SUDT_ACCOUNT_ID, 18).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(0u32.pack())
        .to_id(polyjuice_account.id.pack())
        .nonce(0u32.pack())
        .args(deploy_args.pack())
        .build();

    let mem_block_txs_count = chain.mem_pool().await.mem_block().txs().len();

    let deploy_tx = test_wallet.sign_polyjuice_tx(&state, raw_tx).unwrap();
    let deploy_tx_hash = rpc_server.submit_l2transaction(&deploy_tx).await.unwrap();
    wait_tx_committed(&chain, &deploy_tx_hash, Duration::from_secs(30))
        .await
        .unwrap();

    let system_log = PolyjuiceSystemLog::parse_from_tx_hash(&chain, deploy_tx_hash).unwrap();
    assert_eq!(system_log.status_code, 0);

    let mem_block_txs_count_after = chain.mem_pool().await.mem_block().txs().len();
    assert_eq!(
        mem_block_txs_count_after,
        mem_block_txs_count + 2, // one deploy tx and create account tx
    );

    let test_account_id = state
        .get_account_id_by_script_hash(&test_wallet.account_script_hash())
        .unwrap();
    assert!(test_account_id.is_some());

    let test_registry_address = state
        .get_registry_address_by_script_hash(
            ETH_REGISTRY_ACCOUNT_ID,
            &test_wallet.account_script_hash(),
        )
        .unwrap();
    assert_eq!(
        test_registry_address.as_ref(),
        Some(test_wallet.reg_address())
    );

    let balance_after = state
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, test_wallet.reg_address())
        .unwrap();
    assert!(balance > balance_after);

    // From existing account
    let snap = mem_pool_state.load();
    let mut state = snap.state().unwrap();

    let test_wallet = EthWallet::random(chain.rollup_type_hash());
    let balance: U256 = 1000000u128.into();
    test_wallet.create_account(&mut state, balance).unwrap();
    state.submit_tree_to_mem_block();

    let mem_block_txs_count = chain.mem_pool().await.mem_block().txs().len();

    let deploy_args = SudtErc20ArgsBuilder::deploy(CKB_SUDT_ACCOUNT_ID, 18).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(0u32.pack())
        .to_id(polyjuice_account.id.pack())
        .nonce(0u32.pack())
        .args(deploy_args.pack())
        .build();

    let deploy_tx = test_wallet.sign_polyjuice_tx(&state, raw_tx).unwrap();
    let deploy_tx_hash = rpc_server.submit_l2transaction(&deploy_tx).await.unwrap();
    wait_tx_committed(&chain, &deploy_tx_hash, Duration::from_secs(30))
        .await
        .unwrap();

    let system_log2 = PolyjuiceSystemLog::parse_from_tx_hash(&chain, deploy_tx_hash).unwrap();
    assert_eq!(system_log2.status_code, 0);
    assert!(
        system_log.created_address != system_log2.created_address,
        "should deploy new erc20"
    );

    let mem_block_txs_count_after = chain.mem_pool().await.mem_block().txs().len();
    assert_eq!(
        mem_block_txs_count_after,
        mem_block_txs_count + 1, // one deploy tx
        "should not push create account tx"
    );

    let balance_after = state
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, test_wallet.reg_address())
        .unwrap();
    assert!(balance > balance_after);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_invalid_polyjuice_tx_from_id_zero() {
    let _ = env_logger::builder().is_test(true).try_init();

    let rollup_type_script = Script::default();
    let chain = TestChain::setup(rollup_type_script).await;

    let mem_pool_state = chain.mem_pool_state().await;
    let snap = mem_pool_state.load();
    let mut state = snap.state().unwrap();

    let creator_wallet = EthWallet::random(chain.rollup_type_hash());
    creator_wallet
        .create_account(&mut state, 1000000u128.into())
        .unwrap();
    let rpc_server = RPCServer::build(&chain, Some(creator_wallet.inner))
        .await
        .unwrap();

    let polyjuice_account = PolyjuiceAccount::create(chain.rollup_type_hash(), &mut state).unwrap();
    let deployer_wallet = EthWallet::random(chain.rollup_type_hash());
    deployer_wallet
        .create_account(&mut state, 1000000u128.into())
        .unwrap();
    state.submit_tree_to_mem_block();

    // No creator wallet setup
    let rpc_server_no_creator = RPCServer::build(&chain, None).await.unwrap();
    let deploy_args = SudtErc20ArgsBuilder::deploy(CKB_SUDT_ACCOUNT_ID, 18).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(0u32.pack())
        .to_id(polyjuice_account.id.pack())
        .nonce(0u32.pack())
        .args(deploy_args.pack())
        .build();

    let deploy_tx = deployer_wallet
        .sign_polyjuice_tx(&state, raw_tx.clone())
        .unwrap();
    let err = rpc_server_no_creator
        .submit_l2transaction(&deploy_tx)
        .await
        .unwrap_err();
    eprintln!("err {}", err);

    let expected_err = PolyjuiceTxSenderRecoverError::Internal(anyhow!("no account creator"));
    assert!(err.to_string().contains(&expected_err.to_string()));

    // Mismatch chain id
    let bad_chain_id_raw_tx = raw_tx
        .clone()
        .as_builder()
        .chain_id(chain.chain_id().saturating_add(1).pack())
        .build();
    let bad_chain_id_deploy_tx = deployer_wallet
        .sign_polyjuice_tx(&state, bad_chain_id_raw_tx)
        .unwrap();
    let err = rpc_server
        .submit_l2transaction(&bad_chain_id_deploy_tx)
        .await
        .unwrap_err();
    eprintln!("err {}", err);

    let expected_err = PolyjuiceTxSenderRecoverError::ChainId;
    assert!(err.to_string().contains(&expected_err.to_string()));

    // To script not found
    let bad_to_id_deploy_tx = {
        let raw_tx = deploy_tx.raw().as_builder().to_id(99999u32.pack()).build();
        deploy_tx.clone().as_builder().raw(raw_tx).build()
    };
    let err = rpc_server
        .submit_l2transaction(&bad_to_id_deploy_tx)
        .await
        .unwrap_err();
    eprintln!("err {}", err);

    let expected_err = PolyjuiceTxSenderRecoverError::ToScriptNotFound;
    assert!(err.to_string().contains(&expected_err.to_string()));

    // Invalid signature
    let bad_sig_deploy_tx = deploy_tx
        .as_builder()
        .signature(b"bad signature".pack())
        .build();
    let err = rpc_server
        .submit_l2transaction(&bad_sig_deploy_tx)
        .await
        .unwrap_err();
    eprintln!("err {}", err);

    let expected_err = PolyjuiceTxSenderRecoverError::InvalidSignature(anyhow!(""));
    assert!(err.to_string().contains(&expected_err.to_string()));

    // Insufficient balance
    let snap = mem_pool_state.load();
    let mut state = snap.state().unwrap();

    let test_wallet = EthWallet::random(chain.rollup_type_hash());
    let balance: U256 = MIN_RECOVER_CKB_BALANCE.saturating_sub(1000).into();
    test_wallet.mint_ckb_sudt(&mut state, balance).unwrap();
    state.submit_tree_to_mem_block();

    let deploy_tx = test_wallet.sign_polyjuice_tx(&state, raw_tx).unwrap();
    let err = rpc_server
        .submit_l2transaction(&deploy_tx)
        .await
        .unwrap_err();
    eprintln!("err {}", err);

    let expected_err = PolyjuiceTxSenderRecoverError::InsufficientCkbBalance {
        registry_address: test_wallet.reg_address().to_owned(),
        expect: MIN_RECOVER_CKB_BALANCE.into(),
        got: balance,
    };
    assert!(err.to_string().contains(&expected_err.to_string()));

    // Registered to different script
    let snap = mem_pool_state.load();
    let mut state = snap.state().unwrap();

    let balance: U256 = 9999999u128.into();
    test_wallet.mint_ckb_sudt(&mut state, balance).unwrap();
    state
        .mapping_registry_address_to_script_hash(test_wallet.reg_address().to_owned(), H256::one())
        .unwrap();
    state.submit_tree_to_mem_block();

    let err = rpc_server
        .submit_l2transaction(&deploy_tx)
        .await
        .unwrap_err();
    eprintln!("err {}", err);

    let expected_err = PolyjuiceTxSenderRecoverError::DifferentScript {
        registry_address: test_wallet.reg_address().to_owned(),
        script_hash: H256::one(),
    };
    assert!(err.to_string().contains(&expected_err.to_string()));

    // Account has tx history (nonce isn't zero)
    let test_wallet = EthWallet::random(chain.rollup_type_hash());
    let test_account_id = test_wallet
        .create_account(&mut state, 10000000u128.into())
        .unwrap();
    state.set_nonce(test_account_id, 1).unwrap();
    state.submit_tree_to_mem_block();

    let deploy_args = SudtErc20ArgsBuilder::deploy(CKB_SUDT_ACCOUNT_ID, 18).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(0u32.pack())
        .to_id(polyjuice_account.id.pack())
        .nonce(1u32.pack())
        .args(deploy_args.pack())
        .build();

    let deploy_tx = test_wallet.sign_polyjuice_tx(&state, raw_tx).unwrap();
    let err = rpc_server
        .submit_l2transaction(&deploy_tx)
        .await
        .unwrap_err();
    eprintln!("err {}", err);

    assert!(err.to_string().contains("invalid nonce"));
}
