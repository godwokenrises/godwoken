use anyhow::anyhow;
use ckb_types::prelude::{Builder, Entity};
use gw_common::{builtins::CKB_SUDT_ACCOUNT_ID, registry_address::RegistryAddress, state::State};
use gw_polyjuice_sender_recover::recover::error::PolyjuiceTxSenderRecoverError;
use gw_store::state::traits::JournalDB;
use gw_types::{
    bytes::Bytes,
    h256::*,
    packed::{RawL2Transaction, Script},
    prelude::{Pack, Unpack},
    U256,
};

use crate::testing_tool::{
    chain::TestChain,
    eth_wallet::EthWallet,
    polyjuice::{erc20::SudtErc20ArgsBuilder, PolyjuiceAccount, PolyjuiceSystemLog},
    rpc_server::RPCServer,
};

pub mod block_max_cycles_limit;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_polyjuice_erc20_tx() {
    let _ = env_logger::builder().is_test(true).try_init();

    let rollup_type_script = Script::default();
    let mut chain = TestChain::setup(rollup_type_script).await;
    let rpc_server = RPCServer::build(&chain, None).await.unwrap();

    // Check block producer is valid registry address
    chain
        .produce_block(Default::default(), vec![])
        .await
        .unwrap();
    let block_producer: Bytes = chain.last_valid_block().raw().block_producer().unpack();
    assert!(RegistryAddress::from_slice(&block_producer).is_some());

    let mem_pool_state = chain.mem_pool_state().await;
    let mut state = mem_pool_state.load_state_db();

    let test_wallet = EthWallet::random(chain.rollup_type_hash());
    let test_account_id = test_wallet
        .create_account(&mut state, 1000000u128.into())
        .unwrap();

    let polyjuice_account = PolyjuiceAccount::create(chain.rollup_type_hash(), &mut state).unwrap();

    // Deploy erc20 contract
    let deploy_args = SudtErc20ArgsBuilder::deploy(CKB_SUDT_ACCOUNT_ID, 18).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(test_account_id.pack())
        .to_id(polyjuice_account.id.pack())
        .nonce(0u32.pack())
        .args(deploy_args.pack())
        .build();
    let deploy_tx = test_wallet.sign_polyjuice_tx(&state, raw_tx).unwrap();

    state.finalise().unwrap();
    mem_pool_state.store_state_db(state);
    let run_result = rpc_server.execute_l2transaction(&deploy_tx).await.unwrap();

    let logs = run_result.logs.into_iter().map(Into::into);
    let system_log = PolyjuiceSystemLog::parse_logs(logs).unwrap();
    assert_eq!(system_log.status_code, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_polyjuice_tx_from_id_zero() {
    let _ = env_logger::builder().is_test(true).try_init();

    let rollup_type_script = Script::default();
    let mut chain = TestChain::setup(rollup_type_script).await;
    let rpc_server = RPCServer::build(&chain, None).await.unwrap();

    // Check block producer is valid registry address
    chain
        .produce_block(Default::default(), vec![])
        .await
        .unwrap();
    let block_producer: Bytes = chain.last_valid_block().raw().block_producer().unpack();
    assert!(RegistryAddress::from_slice(&block_producer).is_some());

    let mem_pool_state = chain.mem_pool_state().await;
    let mut state = mem_pool_state.load_state_db();

    let deployer_wallet = EthWallet::random(chain.rollup_type_hash());
    let deployer_id = deployer_wallet
        .create_account(&mut state, 1000000u128.into())
        .unwrap();

    let test_wallet = EthWallet::random(chain.rollup_type_hash());
    let test_balance: U256 = 1000000u128.into();
    test_wallet
        .create_account(&mut state, test_balance)
        .unwrap();

    let polyjuice_account = PolyjuiceAccount::create(chain.rollup_type_hash(), &mut state).unwrap();

    // Deploy erc20 for test
    let deploy_args = SudtErc20ArgsBuilder::deploy(CKB_SUDT_ACCOUNT_ID, 18).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(deployer_id.pack())
        .to_id(polyjuice_account.id.pack())
        .nonce(0u32.pack())
        .args(deploy_args.pack())
        .build();

    let deploy_tx = deployer_wallet.sign_polyjuice_tx(&state, raw_tx).unwrap();
    let deploy_tx_hash: H256 = deploy_tx.hash();

    state.finalise().unwrap();
    mem_pool_state.store_state_db(state);
    {
        let mut mem_pool = chain.mem_pool().await;
        mem_pool.push_transaction(deploy_tx).unwrap();
    }

    let mut state = mem_pool_state.load_state_db();

    let system_log = PolyjuiceSystemLog::parse_from_tx_hash(&chain, deploy_tx_hash).unwrap();
    assert_eq!(system_log.status_code, 0);

    // Check erc20 balance with existing sender
    let erc20_contract_account_id = system_log.contract_account_id(&state).unwrap();
    let balance_args = SudtErc20ArgsBuilder::balance_of(test_wallet.reg_address()).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(0u32.pack())
        .to_id(erc20_contract_account_id.pack())
        .nonce(0u32.pack())
        .args(balance_args.pack())
        .build();

    let balance_tx = test_wallet.sign_polyjuice_tx(&state, raw_tx).unwrap();
    let run_result = rpc_server.execute_l2transaction(&balance_tx).await.unwrap();

    assert_eq!(
        test_balance,
        U256::from_big_endian(run_result.return_data.as_bytes())
    );

    // Check erc20 balance from unregistered sender
    let test_wallet = EthWallet::random(chain.rollup_type_hash());
    let test_balance: U256 = 99999u128.into();
    test_wallet
        .mint_sudt(&mut state, CKB_SUDT_ACCOUNT_ID, test_balance)
        .unwrap();

    let erc20_contract_account_id = system_log.contract_account_id(&state).unwrap();
    let balance_args = SudtErc20ArgsBuilder::balance_of(test_wallet.reg_address()).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(0u32.pack())
        .to_id(erc20_contract_account_id.pack())
        .nonce(0u32.pack())
        .args(balance_args.pack())
        .build();
    let balance_tx = test_wallet.sign_polyjuice_tx(&state, raw_tx).unwrap();

    state.finalise().unwrap();
    mem_pool_state.store_state_db(state);
    let run_result = rpc_server.execute_l2transaction(&balance_tx).await.unwrap();

    assert_eq!(
        test_balance,
        U256::from_big_endian(run_result.return_data.as_bytes())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_invalid_polyjuice_tx_from_id_zero() {
    let _ = env_logger::builder().is_test(true).try_init();

    let rollup_type_script = Script::default();
    let mut chain = TestChain::setup(rollup_type_script).await;
    let rpc_server = RPCServer::build(&chain, None).await.unwrap();

    // Check block producer is valid registry address
    chain
        .produce_block(Default::default(), vec![])
        .await
        .unwrap();
    let block_producer: Bytes = chain.last_valid_block().raw().block_producer().unpack();
    assert!(RegistryAddress::from_slice(&block_producer).is_some());

    let mem_pool_state = chain.mem_pool_state().await;
    let mut state = mem_pool_state.load_state_db();

    let deployer_wallet = EthWallet::random(chain.rollup_type_hash());
    let deployer_id = deployer_wallet
        .create_account(&mut state, 1000000u128.into())
        .unwrap();

    let polyjuice_account = PolyjuiceAccount::create(chain.rollup_type_hash(), &mut state).unwrap();

    // Deploy erc20 for test
    let deploy_args = SudtErc20ArgsBuilder::deploy(CKB_SUDT_ACCOUNT_ID, 18).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(deployer_id.pack())
        .to_id(polyjuice_account.id.pack())
        .nonce(0u32.pack())
        .args(deploy_args.pack())
        .build();

    let deploy_tx = deployer_wallet
        .sign_polyjuice_tx(&state, raw_tx.clone())
        .unwrap();
    let deploy_tx_hash: H256 = deploy_tx.hash();

    state.finalise().unwrap();
    mem_pool_state.store_state_db(state);
    {
        let mut mem_pool = chain.mem_pool().await;
        mem_pool.push_transaction(deploy_tx).unwrap();
    }

    let system_log = PolyjuiceSystemLog::parse_from_tx_hash(&chain, deploy_tx_hash).unwrap();
    assert_eq!(system_log.status_code, 0);

    let state = mem_pool_state.load_state_db();

    let test_wallet = EthWallet::random(chain.rollup_type_hash());
    let erc20_contract_account_id = system_log.contract_account_id(&state).unwrap();
    let balance_args = SudtErc20ArgsBuilder::balance_of(test_wallet.reg_address()).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(0u32.pack())
        .to_id(erc20_contract_account_id.pack())
        .nonce(0u32.pack())
        .args(balance_args.pack())
        .build();
    let balance_tx = test_wallet
        .sign_polyjuice_tx(&state, raw_tx.clone())
        .unwrap();

    // Mismatch chain id
    let bad_chain_id_raw_tx = raw_tx
        .clone()
        .as_builder()
        .chain_id(chain.chain_id().saturating_add(1).pack())
        .build();
    let bad_chain_id_deploy_tx = test_wallet
        .sign_polyjuice_tx(&state, bad_chain_id_raw_tx)
        .unwrap();
    let err = rpc_server
        .execute_l2transaction(&bad_chain_id_deploy_tx)
        .await
        .unwrap_err();
    eprintln!("err {}", err);

    let expected_err = PolyjuiceTxSenderRecoverError::InvalidChainId;
    assert!(err.to_string().contains(&expected_err.to_string()));

    // To script not found
    let bad_to_id_deploy_tx = {
        let raw_tx = balance_tx.raw().as_builder().to_id(99999u32.pack()).build();
        balance_tx.clone().as_builder().raw(raw_tx).build()
    };
    let err = rpc_server
        .execute_l2transaction(&bad_to_id_deploy_tx)
        .await
        .unwrap_err();
    eprintln!("err {}", err);

    let expected_err = PolyjuiceTxSenderRecoverError::ToScriptNotFound;
    assert!(err.to_string().contains(&expected_err.to_string()));

    // Not polyjuice tx
    let bad_to_id_deploy_tx = {
        let raw_tx = balance_tx.raw().as_builder().to_id(1u32.pack()).build();
        balance_tx.clone().as_builder().raw(raw_tx).build()
    };
    let err = rpc_server
        .execute_l2transaction(&bad_to_id_deploy_tx)
        .await
        .unwrap_err();
    eprintln!("err {}", err);

    let expected_err = PolyjuiceTxSenderRecoverError::NotPolyjuiceTx;
    assert!(err.to_string().contains(&expected_err.to_string()));

    // Invalid signature
    let bad_sig_deploy_tx = balance_tx
        .clone()
        .as_builder()
        .signature(b"bad signature".pack())
        .build();
    let err = rpc_server
        .execute_l2transaction(&bad_sig_deploy_tx)
        .await
        .unwrap_err();
    eprintln!("err {}", err);

    let expected_err = PolyjuiceTxSenderRecoverError::InvalidSignature(anyhow!(""));
    assert!(err.to_string().contains(&expected_err.to_string()));

    // Insufficient balance
    let mut state = mem_pool_state.load_state_db();

    let balance = 100u32.into();
    test_wallet.mint_ckb_sudt(&mut state, balance).unwrap();

    state.finalise().unwrap();
    mem_pool_state.store_state_db(state);

    let err = rpc_server
        .execute_l2transaction(&balance_tx)
        .await
        .unwrap_err();
    eprintln!("err {}", err);

    let expected_err = "check balance err";
    assert!(err.to_string().contains(&expected_err));

    // Registered to different script
    let mut state = mem_pool_state.load_state_db();
    state
        .mapping_registry_address_to_script_hash(test_wallet.reg_address().to_owned(), H256::one())
        .unwrap();

    state.finalise().unwrap();
    mem_pool_state.store_state_db(state);

    let err = rpc_server
        .execute_l2transaction(&balance_tx)
        .await
        .unwrap_err();
    eprintln!("err {}", err);

    let expected_err = PolyjuiceTxSenderRecoverError::DifferentScript {
        registry_address: test_wallet.registry_address,
        script_hash: H256::one(),
    };
    assert!(err.to_string().contains(&expected_err.to_string()));
}
