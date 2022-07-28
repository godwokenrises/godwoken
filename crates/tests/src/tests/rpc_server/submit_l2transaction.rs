use std::time::Duration;

use ckb_types::prelude::{Builder, Entity};
use gw_common::{
    blake2b::new_blake2b,
    builtins::{CKB_SUDT_ACCOUNT_ID, ETH_REGISTRY_ACCOUNT_ID},
    h256_ext::H256Ext,
    state::State,
    H256,
};
use gw_types::{
    bytes::Bytes,
    packed::{Fee, RawL2Transaction, SUDTArgs, SUDTTransfer, Script},
    prelude::{Pack, Unpack},
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

    mem_pool_state.store(snap.into());
    let deploy_tx_hash = rpc_server
        .submit_l2transaction(&deploy_tx)
        .await
        .unwrap()
        .unwrap();
    wait_tx_committed(&chain, &deploy_tx_hash, Duration::from_secs(30))
        .await
        .unwrap();

    let system_log = PolyjuiceSystemLog::parse_from_tx_hash(&chain, deploy_tx_hash).unwrap();
    assert_eq!(system_log.status_code, 0);

    let snap = mem_pool_state.load();
    let state = snap.state().unwrap();

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
    let transfer_tx_hash = rpc_server
        .submit_l2transaction(&transfer_tx)
        .await
        .unwrap()
        .unwrap();
    wait_tx_committed(&chain, &transfer_tx_hash, Duration::from_secs(30))
        .await
        .unwrap();

    let system_log = PolyjuiceSystemLog::parse_from_tx_hash(&chain, deploy_tx_hash).unwrap();
    assert_eq!(system_log.status_code, 0);

    let snap = mem_pool_state.load();
    let state = snap.state().unwrap();

    let balance = state
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, to_wallet.reg_address())
        .unwrap();
    assert_eq!(balance, amount);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_in_queue_query_with_signature_hash() {
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

    let deploy_tx_hash = {
        let id = state.get_account_count().unwrap();
        let raw = raw_tx.clone().as_builder().from_id(id.pack()).build();
        raw.hash().into()
    };
    let deploy_tx = test_wallet.sign_polyjuice_tx(&state, raw_tx).unwrap();

    mem_pool_state.store(snap.into());
    let signature_hash = {
        let mut hasher = new_blake2b();
        let sig: Bytes = deploy_tx.signature().unpack();
        hasher.update(&sig);
        let mut hash = [0u8; 32];
        hasher.finalize(&mut hash);
        H256::from(hash)
    };

    rpc_server.submit_l2transaction(&deploy_tx).await.unwrap();

    let is_in_queue = rpc_server
        .is_request_in_queue(signature_hash)
        .await
        .unwrap();
    assert!(is_in_queue);

    wait_tx_committed(&chain, &deploy_tx_hash, Duration::from_secs(30))
        .await
        .unwrap();

    let system_log = PolyjuiceSystemLog::parse_from_tx_hash(&chain, deploy_tx_hash).unwrap();
    assert_eq!(system_log.status_code, 0);
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

    let deploy_tx_hash = {
        let id = state.get_account_count().unwrap();
        let raw = raw_tx.clone().as_builder().from_id(id.pack()).build();
        raw.hash().into()
    };
    let deploy_tx = test_wallet.sign_polyjuice_tx(&state, raw_tx).unwrap();

    mem_pool_state.store(snap.into());
    rpc_server.submit_l2transaction(&deploy_tx).await.unwrap();
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

    let snap = mem_pool_state.load();
    let state = snap.state().unwrap();

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
    let expected_id = test_wallet.create_account(&mut state, balance).unwrap();
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

    let deploy_tx_hash = {
        let id = expected_id;
        let raw = raw_tx.clone().as_builder().from_id(id.pack()).build();
        raw.hash().into()
    };
    let deploy_tx = test_wallet.sign_polyjuice_tx(&state, raw_tx).unwrap();
    mem_pool_state.store(snap.into());

    rpc_server.submit_l2transaction(&deploy_tx).await.unwrap();
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

    let snap = mem_pool_state.load();
    let state = snap.state().unwrap();

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
    let deployer_id = deployer_wallet
        .create_account(&mut state, 1000000u128.into())
        .unwrap();
    state.submit_tree_to_mem_block();

    let expected_txs_count = chain.mem_pool().await.mem_block().txs().len();

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

    mem_pool_state.store(snap.into());
    let err = rpc_server_no_creator
        .submit_l2transaction(&deploy_tx)
        .await
        .unwrap_err();
    eprintln!("err {}", err);

    let expected_err = "tx from zero is disabled";
    assert!(err.to_string().contains(&expected_err));

    let snap = mem_pool_state.load();
    let state = snap.state().unwrap();

    // Mismatch chain id
    let bad_chain_id_raw_tx = raw_tx
        .clone()
        .as_builder()
        .chain_id(chain.chain_id().saturating_add(1).pack())
        .build();
    let bad_chain_id_deploy_tx = deployer_wallet
        .sign_polyjuice_tx(&state, bad_chain_id_raw_tx)
        .unwrap();
    rpc_server
        .submit_l2transaction(&bad_chain_id_deploy_tx)
        .await
        .unwrap();

    // To script not found
    let bad_to_id_deploy_tx = {
        let raw_tx = deploy_tx.raw().as_builder().to_id(99999u32.pack()).build();
        deploy_tx.clone().as_builder().raw(raw_tx).build()
    };
    rpc_server
        .submit_l2transaction(&bad_to_id_deploy_tx)
        .await
        .unwrap();

    // Not polyjuice tx
    let test_wallet = EthWallet::random(chain.rollup_type_hash());
    let bad_to_id_deploy_tx = {
        let sudt_transfer = SUDTTransfer::new_builder()
            .to_address(test_wallet.reg_address().to_bytes().pack())
            .amount(U256::one().pack())
            .fee(
                Fee::new_builder()
                    .registry_id(ETH_REGISTRY_ACCOUNT_ID.pack())
                    .amount(1u128.pack())
                    .build(),
            )
            .build();
        let sudt_args = SUDTArgs::new_builder().set(sudt_transfer).build();

        let raw_tx = RawL2Transaction::new_builder()
            .from_id(0u32.pack())
            .to_id(1u32.pack())
            .nonce(0u32.pack())
            .args(sudt_args.as_bytes().pack())
            .build();

        deploy_tx.clone().as_builder().raw(raw_tx).build()
    };
    rpc_server
        .submit_l2transaction(&bad_to_id_deploy_tx)
        .await
        .unwrap();

    // Invalid signature
    let bad_sig_deploy_tx = deploy_tx
        .clone()
        .as_builder()
        .signature(b"bad signature".pack())
        .build();
    rpc_server
        .submit_l2transaction(&bad_sig_deploy_tx)
        .await
        .unwrap();

    // Insufficient balance
    let snap = mem_pool_state.load();
    let mut state = snap.state().unwrap();

    let test_wallet = EthWallet::random(chain.rollup_type_hash());
    let balance: U256 = 100u32.into();
    test_wallet.mint_ckb_sudt(&mut state, balance).unwrap();
    state.submit_tree_to_mem_block();

    let insufficient_balance_deploy_tx = test_wallet
        .sign_polyjuice_tx(&state, raw_tx.clone())
        .unwrap();
    mem_pool_state.store(snap.into());

    rpc_server
        .submit_l2transaction(&insufficient_balance_deploy_tx)
        .await
        .unwrap();

    // Registered to different script
    let snap = mem_pool_state.load();
    let mut state = snap.state().unwrap();

    let test_wallet = EthWallet::random(chain.rollup_type_hash());
    let balance: U256 = 9999999u128.into();
    test_wallet.mint_ckb_sudt(&mut state, balance).unwrap();
    state
        .mapping_registry_address_to_script_hash(test_wallet.reg_address().to_owned(), H256::one())
        .unwrap();
    state.submit_tree_to_mem_block();

    let different_script_deploy_tx = test_wallet.sign_polyjuice_tx(&state, raw_tx).unwrap();
    mem_pool_state.store(snap.into());

    rpc_server
        .submit_l2transaction(&different_script_deploy_tx)
        .await
        .unwrap();

    let snap = mem_pool_state.load();
    let mut state = snap.state().unwrap();

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
    let bad_deploy_tx = test_wallet.sign_polyjuice_tx(&state, raw_tx).unwrap();

    mem_pool_state.store(snap.into());
    let err = rpc_server
        .submit_l2transaction(&bad_deploy_tx)
        .await
        .unwrap_err();
    eprintln!("err {}", err);

    assert!(err.to_string().contains("invalid nonce"));

    // Use valid tx to check fee queue
    let deploy_tx = {
        let id = deployer_id;
        let raw = deploy_tx.raw().as_builder().from_id(id.pack()).build();
        deploy_tx.as_builder().raw(raw).build()
    };
    let tx_hash = rpc_server
        .submit_l2transaction(&deploy_tx)
        .await
        .unwrap()
        .unwrap();
    wait_tx_committed(&chain, &tx_hash, Duration::from_secs(30))
        .await
        .unwrap();

    let txs_count = chain.mem_pool().await.mem_block().txs().len();
    assert_eq!(
        expected_txs_count + 1,
        txs_count,
        "unrecoverable txs should not be committed"
    );
}
