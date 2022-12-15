use ckb_types::prelude::{Builder, Entity};
use gw_common::{
    builtins::{CKB_SUDT_ACCOUNT_ID, ETH_REGISTRY_ACCOUNT_ID, RESERVED_ACCOUNT_ID},
    ckb_decimal::CKBCapacity,
    registry_address::RegistryAddress,
    state::State,
};
use gw_generator::account_lock_manage::secp256k1::Secp256k1Eth;
use gw_mem_pool::account_creator::{AccountCreator, MIN_BALANCE};
use gw_types::{
    bytes::Bytes,
    h256::*,
    packed::{
        CreateAccount, DepositInfoVec, DepositRequest, Fee, L2Transaction, MetaContractArgs,
        RawL2Transaction, Script,
    },
    prelude::{Pack, Unpack},
    U256,
};

use crate::testing_tool::{
    chain::{into_deposit_info_cell, TestChain},
    eth_wallet::EthWallet,
    polyjuice::{erc20::SudtErc20ArgsBuilder, PolyjuiceAccount, PolyjuiceSystemLog},
};

const META_CONTRACT_ACCOUNT_ID: u32 = RESERVED_ACCOUNT_ID;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_create_account_for_ckb_transfer_new_address_recipient() {
    let _ = env_logger::builder().is_test(true).try_init();

    let rollup_type_script = Script::default();
    let mut chain = TestChain::setup(rollup_type_script).await;

    // Deposit test account
    let test_wallet = EthWallet::random(chain.rollup_type_hash());
    let deposit = DepositRequest::new_builder()
        .capacity((MIN_BALANCE * 1000).pack())
        .sudt_script_hash(H256::zero().pack())
        .amount(0.pack())
        .script(test_wallet.account_script().to_owned())
        .registry_id(ETH_REGISTRY_ACCOUNT_ID.pack())
        .build();
    let deposit_info_vec = DepositInfoVec::new_builder()
        .push(into_deposit_info_cell(chain.inner.generator().rollup_context(), deposit).pack())
        .build();
    chain.produce_block(deposit_info_vec, vec![]).await.unwrap();

    let block_producer: Bytes = chain.last_valid_block().raw().block_producer().unpack();
    assert!(RegistryAddress::from_slice(&block_producer).is_some());

    let mem_pool_state = chain.mem_pool_state().await;
    let state = mem_pool_state.load_state_db();

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

    let to_wallet = EthWallet::random(chain.rollup_type_hash());
    let amount: U256 = CKBCapacity::from_layer1(MIN_BALANCE).to_layer2();

    let transfer_args = SudtErc20ArgsBuilder::transfer(to_wallet.reg_address(), amount).finish();
    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(test_account_id.pack())
        .to_id(erc20_contract_account_id.pack())
        .nonce(2u32.pack())
        .args(transfer_args.pack())
        .build();

    let transfer_tx = test_wallet.sign_polyjuice_tx(&state, raw_tx).unwrap();
    let account_creator =
        AccountCreator::create(chain.inner.generator().rollup_context(), test_wallet.inner)
            .unwrap();
    {
        let mut mem_pool = chain.mem_pool().await;
        mem_pool.set_account_creator(account_creator);
        mem_pool.push_transaction(transfer_tx).unwrap();
    }

    let system_log = PolyjuiceSystemLog::parse_from_tx_hash(&chain, deploy_tx_hash).unwrap();
    assert_eq!(system_log.status_code, 0);

    let state = mem_pool_state.load_state_db();

    let balance = state
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, to_wallet.reg_address())
        .unwrap();
    assert_eq!(balance, amount);

    let account_non_exists = state
        .get_script_hash_by_registry_address(to_wallet.reg_address())
        .unwrap()
        .is_none();
    assert!(account_non_exists);

    chain
        .produce_block(Default::default(), vec![])
        .await
        .unwrap();

    let state = mem_pool_state.load_state_db();

    let balance = state
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, to_wallet.reg_address())
        .unwrap();
    assert_eq!(balance, amount);

    let account_exists = state
        .get_script_hash_by_registry_address(to_wallet.reg_address())
        .unwrap()
        .is_some();
    assert!(account_exists);
}
