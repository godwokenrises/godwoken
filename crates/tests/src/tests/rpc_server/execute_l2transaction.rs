use ckb_types::prelude::{Builder, Entity};
use gw_common::{builtins::CKB_SUDT_ACCOUNT_ID, registry_address::RegistryAddress, H256};
use gw_store::traits::chain_store::ChainStore;
use gw_types::{
    bytes::Bytes,
    packed::{RawL2Transaction, Script},
    prelude::{Pack, Unpack},
    U256,
};

use crate::testing_tool::{
    chain::{setup_chain, sync_dummy_block},
    eth_wallet::EthWallet,
    polyjuice::{erc20::SudtErc20ArgsBuilder, PolyjuiceAccount, PolyjuiceSystemLog},
    rpc_server::RPCServer,
};

#[tokio::test(flavor = "multi_thread")]
async fn test_polyjuice_erc20_tx() {
    let _ = env_logger::builder().is_test(true).try_init();

    let rollup_type_script = Script::default();
    let rollup_script_hash: H256 = rollup_type_script.hash().into();

    let mut chain = setup_chain(rollup_type_script.clone()).await;
    let chain_id = {
        let config = &chain.generator().rollup_context().rollup_config;
        config.chain_id().unpack()
    };
    let rpc_server = RPCServer::build(&chain, rollup_type_script.clone(), None)
        .await
        .unwrap();

    // Produce a block to override block producer address in genesis block
    sync_dummy_block(&mut chain, rollup_type_script)
        .await
        .unwrap();
    let last_valid_block = chain
        .store()
        .get_snapshot()
        .get_last_valid_tip_block()
        .unwrap();

    // Check block producer is valid registry address
    let block_producer: Bytes = last_valid_block.raw().block_producer().unpack();
    assert!(RegistryAddress::from_slice(&block_producer).is_some());

    // Deploy erc20 contract
    let mem_pool_state = {
        let mem_pool = chain.mem_pool().as_ref().unwrap().lock().await;
        mem_pool.mem_pool_state()
    };
    let snap = mem_pool_state.load();
    let mut state = snap.state().unwrap();

    let polyjuice_account = PolyjuiceAccount::create(rollup_script_hash, &mut state).unwrap();
    let creator_wallet = EthWallet::random(rollup_script_hash);
    let creator_account_id = creator_wallet
        .create_account(&mut state, 1000000u128.into())
        .unwrap();
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

    let run_result = rpc_server
        .execute_l2transaction(&erc20_deploy_tx)
        .await
        .unwrap();

    let logs = run_result.logs.into_iter().map(Into::into);
    let system_log = PolyjuiceSystemLog::parse_logs(logs).unwrap();
    assert_eq!(system_log.status_code, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_polyjuice_tx_from_id_zero() {
    let _ = env_logger::builder().is_test(true).try_init();

    let rollup_type_script = Script::default();
    let rollup_script_hash: H256 = rollup_type_script.hash().into();

    let mut chain = setup_chain(rollup_type_script.clone()).await;
    let chain_id = {
        let config = &chain.generator().rollup_context().rollup_config;
        config.chain_id().unpack()
    };
    let rpc_server = RPCServer::build(&chain, rollup_type_script.clone(), None)
        .await
        .unwrap();

    // Produce a block to override block producer address in genesis block
    sync_dummy_block(&mut chain, rollup_type_script)
        .await
        .unwrap();
    let last_valid_block = chain
        .store()
        .get_snapshot()
        .get_last_valid_tip_block()
        .unwrap();

    // Check block producer is valid registry address
    let block_producer: Bytes = last_valid_block.raw().block_producer().unpack();
    assert!(RegistryAddress::from_slice(&block_producer).is_some());

    // Deploy erc20 contract
    let mem_pool_state = {
        let mem_pool = chain.mem_pool().as_ref().unwrap().lock().await;
        mem_pool.mem_pool_state()
    };
    let snap = mem_pool_state.load();
    let mut state = snap.state().unwrap();

    let polyjuice_account = PolyjuiceAccount::create(rollup_script_hash, &mut state).unwrap();
    let deployer_wallet = EthWallet::random(rollup_script_hash);
    let deployer_id = deployer_wallet
        .create_account(&mut state, 1000000u128.into())
        .unwrap();
    state.submit_tree_to_mem_block();

    let deploy_args = SudtErc20ArgsBuilder::deploy(CKB_SUDT_ACCOUNT_ID, 18).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain_id.pack())
        .from_id(deployer_id.pack())
        .to_id(polyjuice_account.id.pack())
        .nonce(0u32.pack())
        .args(deploy_args.pack())
        .build();
    let erc20_deploy_tx = deployer_wallet.sign_polyjuice_tx(&state, raw_tx).unwrap();
    let erc20_deploy_tx_hash: H256 = erc20_deploy_tx.hash().into();
    {
        let mut mem_pool = chain.mem_pool().as_ref().unwrap().lock().await;
        mem_pool.push_transaction(erc20_deploy_tx).await.unwrap();
    }

    let system_log = PolyjuiceSystemLog::parse_from_tx_hash(&chain, erc20_deploy_tx_hash).unwrap();
    assert_eq!(system_log.status_code, 0);

    // Check erc20 balance from unregistered sender
    let test_wallet = EthWallet::random(rollup_script_hash);
    let test_balance: U256 = 99999u128.into();
    test_wallet
        .mint_sudt(&mut state, CKB_SUDT_ACCOUNT_ID, test_balance)
        .unwrap();

    let erc20_contract_account_id = system_log.contract_account_id(&state).unwrap();
    let balance_args = SudtErc20ArgsBuilder::balance_of(&test_wallet.registry_address).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain_id.pack())
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
}
