use ckb_types::prelude::{Builder, Entity};
use gw_common::{builtins::CKB_SUDT_ACCOUNT_ID, registry_address::RegistryAddress};
use gw_config::{MemBlockConfig, MemPoolConfig};
use gw_store::state::traits::JournalDB;
use gw_types::{
    bytes::Bytes,
    packed::{RawL2Transaction, Script},
    prelude::{Pack, Unpack},
};

use crate::testing_tool::{
    chain::TestChain,
    eth_wallet::EthWallet,
    polyjuice::{erc20::SudtErc20ArgsBuilder, PolyjuiceAccount, PolyjuiceSystemLog},
    rpc_server::RPCServer,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_block_max_cycles_limit() {
    let _ = env_logger::builder().is_test(true).try_init();

    let mem_pool_config = MemPoolConfig {
        mem_block: MemBlockConfig {
            max_cycles_limit: crate::tests::rpc_server::BLOCK_MAX_CYCLES_LIMIT,
            ..Default::default()
        },
        ..Default::default()
    };

    let rollup_type_script = Script::default();
    let mut chain = {
        let chain = TestChain::setup(rollup_type_script).await;
        chain.update_mem_pool_config(mem_pool_config.clone()).await
    };
    let rpc_server = {
        let mut args = RPCServer::default_registry_args(
            &chain.inner,
            chain.rollup_type_script.to_owned(),
            None,
        );
        args.mem_pool_config = mem_pool_config;
        RPCServer::build_from_registry_args(args).await.unwrap()
    };

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
    let reg_addr_bytes = test_wallet.reg_address().to_bytes().into();

    state.finalise().unwrap();
    mem_pool_state.store_state_db(state);
    let run_result = rpc_server
        .execute_raw_l2transaction(&raw_tx, None, Some(reg_addr_bytes))
        .await
        .unwrap();

    let logs = run_result.logs.into_iter().map(Into::into);
    let system_log = PolyjuiceSystemLog::parse_logs(logs).unwrap();
    assert_eq!(system_log.status_code, 0);

    // Test TransactionError::ExceededBlockMaxCycles
    let mem_pool_config = MemPoolConfig {
        mem_block: MemBlockConfig {
            max_cycles_limit: 1000,
            ..Default::default()
        },
        ..Default::default()
    };

    let rollup_type_script = Script::default();
    let chain = {
        let chain = TestChain::setup(rollup_type_script).await;
        chain.update_mem_pool_config(mem_pool_config.clone()).await
    };
    let rpc_server = {
        let mut args = RPCServer::default_registry_args(
            &chain.inner,
            chain.rollup_type_script.to_owned(),
            None,
        );
        args.mem_pool_config = mem_pool_config;
        RPCServer::build_from_registry_args(args).await.unwrap()
    };

    let mem_pool_state = chain.mem_pool_state().await;
    let mut state = mem_pool_state.load_state_db();

    let test_wallet = EthWallet::random(chain.rollup_type_hash());
    let test_account_id = test_wallet
        .create_account(&mut state, 1000000u128.into())
        .unwrap();

    let polyjuice_account = PolyjuiceAccount::create(chain.rollup_type_hash(), &mut state).unwrap();

    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(test_account_id.pack())
        .to_id(polyjuice_account.id.pack())
        .nonce(0u32.pack())
        .args(deploy_args.pack())
        .build();
    let reg_addr_bytes = test_wallet.reg_address().to_bytes().into();

    state.finalise().unwrap();
    mem_pool_state.store_state_db(state);
    let err = rpc_server
        .execute_raw_l2transaction(&raw_tx, None, Some(reg_addr_bytes))
        .await
        .unwrap_err();
    eprintln!("err {}", err);

    let expected_err = "Exceeded max block cycles";
    assert!(err.to_string().contains(expected_err));
}
