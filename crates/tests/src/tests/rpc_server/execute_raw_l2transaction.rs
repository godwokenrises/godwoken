use gw_common::{
    builtins::{CKB_SUDT_ACCOUNT_ID, ETH_REGISTRY_ACCOUNT_ID, RESERVED_ACCOUNT_ID},
    registry_address::RegistryAddress,
    state::State,
};
use gw_generator::account_lock_manage::secp256k1::Secp256k1Eth;
use gw_store::state::{history::history_state::RWConfig, traits::JournalDB, BlockStateDB};
use gw_types::{
    bytes::Bytes,
    h256::*,
    packed::{
        CreateAccount, DepositInfoVec, DepositRequest, Fee, L2Transaction, MetaContractArgs,
        RawL2Transaction, Script,
    },
    prelude::*,
    U256,
};

use crate::testing_tool::{
    chain::{into_deposit_info_cell, TestChain},
    eth_wallet::EthWallet,
    polyjuice::{erc20::SudtErc20ArgsBuilder, PolyjuiceAccount, PolyjuiceSystemLog},
    rpc_server::RPCServer,
};

pub mod block_max_cycles_limit;

const META_CONTRACT_ACCOUNT_ID: u32 = RESERVED_ACCOUNT_ID;

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

    let system_log = PolyjuiceSystemLog::parse_from_tx_hash(&chain, deploy_tx_hash).unwrap();
    assert_eq!(system_log.status_code, 0);

    let mut state = mem_pool_state.load_state_db();

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

    let reg_addr_bytes = test_wallet.reg_address().to_bytes().into();
    let run_result = rpc_server
        .execute_raw_l2transaction(&raw_tx, None, Some(reg_addr_bytes))
        .await
        .unwrap();

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

    state.finalise().unwrap();
    mem_pool_state.store_state_db(state);
    let state = mem_pool_state.load_state_db();

    let erc20_contract_account_id = system_log.contract_account_id(&state).unwrap();
    let balance_args = SudtErc20ArgsBuilder::balance_of(&test_wallet.registry_address).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(0u32.pack())
        .to_id(erc20_contract_account_id.pack())
        .nonce(0u32.pack())
        .args(balance_args.pack())
        .build();

    let reg_addr_bytes = test_wallet.reg_address().to_bytes().into();
    let run_result = rpc_server
        .execute_raw_l2transaction(&raw_tx, None, Some(reg_addr_bytes))
        .await
        .unwrap();

    assert_eq!(
        test_balance,
        U256::from_big_endian(run_result.return_data.as_bytes())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_polyjuice_tx_from_id_zero_with_block_number() {
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

    // Deploy erc20 contract for test
    let mem_pool_state = chain.mem_pool_state().await;
    let state = mem_pool_state.load_state_db();

    let pre_block1_balance = state
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, test_wallet.reg_address())
        .unwrap();

    let test_account_id = state
        .get_account_id_by_script_hash(&test_wallet.account_script_hash())
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
        .from_id(test_account_id.pack())
        .to_id(META_CONTRACT_ACCOUNT_ID.pack())
        .nonce(0u32.pack())
        .args(args.as_bytes().pack())
        .build();

    let signing_message = Secp256k1Eth::eip712_signing_message(
        chain.chain_id(),
        &raw_l2tx,
        test_wallet.reg_address().to_owned(),
        meta_contract_script_hash,
    )
    .unwrap();
    let sign = test_wallet.sign_message(signing_message).unwrap();

    let deploy_tx = L2Transaction::new_builder()
        .raw(raw_l2tx)
        .signature(sign.pack())
        .build();
    {
        let mut mem_pool = chain.mem_pool().await;
        mem_pool.push_transaction(deploy_tx).unwrap();
    }

    let state = mem_pool_state.load_state_db();

    // Depoly erc20 contract
    let polyjuice_account_id = state
        .get_account_id_by_script_hash(&polyjuice_account.hash())
        .unwrap()
        .unwrap();
    let deploy_args = SudtErc20ArgsBuilder::deploy(CKB_SUDT_ACCOUNT_ID, 18).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(test_account_id.pack())
        .to_id(polyjuice_account_id.pack())
        .nonce(1u32.pack())
        .args(deploy_args.pack())
        .build();

    let deploy_tx = test_wallet.sign_polyjuice_tx(&state, raw_tx).unwrap();
    let deploy_tx_hash: H256 = deploy_tx.hash();

    {
        let mut mem_pool = chain.mem_pool().await;
        mem_pool.push_transaction(deploy_tx).unwrap();
    }

    let system_log = PolyjuiceSystemLog::parse_from_tx_hash(&chain, deploy_tx_hash).unwrap();
    assert_eq!(system_log.status_code, 0);

    let state = mem_pool_state.load_state_db();

    let erc20_contract_account_id = system_log.contract_account_id(&state).unwrap();
    let erc20_contract_script_hash = state.get_script_hash(erc20_contract_account_id).unwrap();
    assert!(!erc20_contract_script_hash.is_zero());

    let post_block1_balance = state
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, test_wallet.reg_address())
        .unwrap();

    chain
        .produce_block(Default::default(), vec![])
        .await
        .unwrap();

    // Check block producer is valid registry address
    let block_producer: Bytes = chain.last_valid_block().raw().block_producer().unpack();
    assert!(RegistryAddress::from_slice(&block_producer).is_some());

    let state = mem_pool_state.load_state_db();

    let expect_polyjuice_account_id = state
        .get_account_id_by_script_hash(&polyjuice_account.hash())
        .unwrap()
        .unwrap();
    assert_eq!(expect_polyjuice_account_id, polyjuice_account_id);

    let expect_erc20_contract_script_hash =
        state.get_script_hash(erc20_contract_account_id).unwrap();
    assert_eq!(
        expect_erc20_contract_script_hash,
        erc20_contract_script_hash
    );

    // Transfer for test
    let to_wallet = EthWallet::random(chain.rollup_type_hash());
    let transfer_amount: U256 = 40000u128.into();
    assert!(post_block1_balance > transfer_amount);

    let transfer_args =
        SudtErc20ArgsBuilder::transfer(to_wallet.reg_address(), transfer_amount).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(test_account_id.pack())
        .to_id(erc20_contract_account_id.pack())
        .nonce(2u32.pack())
        .args(transfer_args.pack())
        .build();

    let transfer_tx = test_wallet.sign_polyjuice_tx(&state, raw_tx).unwrap();
    {
        let mut mem_pool = chain.mem_pool().await;
        mem_pool.push_transaction(transfer_tx).unwrap();
    }

    let state = mem_pool_state.load_state_db();

    let to_balance = state
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, to_wallet.reg_address())
        .unwrap();
    assert_eq!(to_balance, transfer_amount);

    let to_balance_of_args = SudtErc20ArgsBuilder::balance_of(to_wallet.reg_address()).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(0u32.pack())
        .to_id(erc20_contract_account_id.pack())
        .nonce(0u32.pack())
        .args(to_balance_of_args.pack())
        .build();

    let to_reg_addr_bytes: Bytes = to_wallet.reg_address().to_bytes().into();
    let run_result = rpc_server
        .execute_raw_l2transaction(&raw_tx, None, Some(to_reg_addr_bytes))
        .await
        .unwrap();
    assert_eq!(
        to_balance,
        U256::from_big_endian(run_result.return_data.as_bytes())
    );

    let post_block2_balance = state
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, test_wallet.reg_address())
        .unwrap();
    assert!(post_block1_balance > post_block2_balance);

    // Use block 2 to check post block 1 state
    chain
        .produce_block(Default::default(), vec![])
        .await
        .unwrap();

    let mut db = chain.store().begin_transaction();
    let pre_block1_hist_state =
        BlockStateDB::from_store(&mut db, RWConfig::history_block(1)).unwrap();
    let hist_balance = pre_block1_hist_state
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, test_wallet.reg_address())
        .unwrap();
    assert_eq!(hist_balance, pre_block1_balance);

    let post_block1_hist_state =
        BlockStateDB::from_store(&mut db, RWConfig::history_block(2)).unwrap();
    let hist_balance = post_block1_hist_state
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, test_wallet.reg_address())
        .unwrap();
    assert_eq!(hist_balance, post_block1_balance);

    let test_balance_of_args = SudtErc20ArgsBuilder::balance_of(test_wallet.reg_address()).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(0u32.pack())
        .to_id(erc20_contract_account_id.pack())
        .nonce(2u32.pack())
        .args(test_balance_of_args.pack())
        .build();

    let test_reg_addr_bytes: Bytes = test_wallet.reg_address().to_bytes().into();
    let run_result = rpc_server
        .execute_raw_l2transaction(&raw_tx, Some(2), Some(test_reg_addr_bytes))
        .await
        .unwrap();
    assert_eq!(
        post_block1_balance,
        U256::from_big_endian(run_result.return_data.as_bytes())
    );

    // Use block 3 to check post block 2 state
    chain
        .produce_block(Default::default(), vec![])
        .await
        .unwrap();

    let to_balance_of_args = SudtErc20ArgsBuilder::balance_of(to_wallet.reg_address()).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(0u32.pack())
        .to_id(erc20_contract_account_id.pack())
        .nonce(0u32.pack())
        .args(to_balance_of_args.pack())
        .build();

    let to_reg_addr_bytes: Bytes = to_wallet.reg_address().to_bytes().into();
    let run_result = rpc_server
        .execute_raw_l2transaction(&raw_tx, Some(3), Some(to_reg_addr_bytes))
        .await
        .unwrap();
    assert_eq!(
        transfer_amount,
        U256::from_big_endian(run_result.return_data.as_bytes())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_invalid_registry_address() {
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

    let system_log = PolyjuiceSystemLog::parse_from_tx_hash(&chain, deploy_tx_hash).unwrap();
    assert_eq!(system_log.status_code, 0);

    let state = mem_pool_state.load_state_db();

    // No registry address
    let erc20_contract_account_id = system_log.contract_account_id(&state).unwrap();
    let balance_args = SudtErc20ArgsBuilder::balance_of(test_wallet.reg_address()).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(0u32.pack())
        .to_id(erc20_contract_account_id.pack())
        .nonce(0u32.pack())
        .args(balance_args.pack())
        .build();

    let err = rpc_server
        .execute_raw_l2transaction(&raw_tx, None, None)
        .await
        .unwrap_err();

    eprintln!("err {}", err);
    assert!(err.to_string().contains("no registry address"));

    // Invalid registry address
    let err = rpc_server
        .execute_raw_l2transaction(&raw_tx, None, Some(Bytes::from_static(b"invalid")))
        .await
        .unwrap_err();

    eprintln!("err {}", err);
    assert!(err.to_string().contains("Invalid registry address"));
}
