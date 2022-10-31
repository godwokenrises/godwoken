#![allow(clippy::mutable_key_type)]

use anyhow::Result;
use ckb_types::prelude::{Builder, Entity};
use gw_common::state::State;
use gw_polyjuice_sender_recover::recover::{
    eth_account_creator::EthAccountCreator, eth_recover::EthAccountContext,
    eth_sender::PolyjuiceTxEthSender,
};
use gw_types::{
    packed::{RawL2Transaction, Script},
    prelude::Pack,
};

use crate::testing_tool::{
    chain::{TestChain, ETH_ACCOUNT_LOCK_CODE_HASH, POLYJUICE_VALIDATOR_CODE_HASH},
    eth_wallet::EthWallet,
    polyjuice::{PolyjuiceAccount, PolyjuiceArgsBuilder},
};

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_eth_account_creator() {
    let _ = env_logger::builder().is_test(true).try_init();

    let rollup_type_script = Script::default();
    let chain = TestChain::setup(rollup_type_script).await;

    let mem_pool_state = chain.mem_pool_state().await;
    let mut state = mem_pool_state.load_state_db();

    let creator_wallet = EthWallet::random(chain.rollup_type_hash());
    creator_wallet
        .create_account(&mut state, 10000u128.into())
        .unwrap();

    let polyjuice_account = PolyjuiceAccount::create(chain.rollup_type_hash(), &mut state).unwrap();

    let account_ctx = EthAccountContext::new(
        chain.chain_id(),
        chain.rollup_type_hash(),
        (*ETH_ACCOUNT_LOCK_CODE_HASH).into(),
        (*POLYJUICE_VALIDATOR_CODE_HASH).into(),
    );
    let eth_account_creator =
        EthAccountCreator::create(&account_ctx, creator_wallet.inner).unwrap();

    let new_users_count = 5;
    let new_users_wallet: Vec<_> = (0..new_users_count)
        .map(|_| EthWallet::random(chain.rollup_type_hash()))
        .collect();

    for wallet in new_users_wallet.iter() {
        wallet.mint_ckb_sudt(&mut state, 1u128.into()).unwrap();
    }

    let polyjuice_create_args = PolyjuiceArgsBuilder::default()
        .create(true)
        .data(b"POLYJUICEcontract".to_vec())
        .finish();

    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(0u32.pack())
        .to_id(polyjuice_account.id.pack())
        .nonce(0u32.pack())
        .args(polyjuice_create_args.pack())
        .build();

    let txs = new_users_wallet
        .iter()
        .map(|wallet| wallet.sign_polyjuice_tx(&state, raw_tx.clone()))
        .collect::<Result<Vec<_>>>()
        .unwrap();

    let recovered_account_scripts = txs
        .iter()
        .filter_map(
            |tx| match PolyjuiceTxEthSender::recover(&account_ctx, &state, tx).ok() {
                Some(PolyjuiceTxEthSender::New { account_script, .. }) => Some(account_script),
                _ => None,
            },
        )
        .collect::<Vec<_>>();
    assert_eq!(recovered_account_scripts.len(), new_users_wallet.len());

    let batch_create_tx = eth_account_creator
        .build_batch_create_tx(&state, recovered_account_scripts)
        .unwrap();

    mem_pool_state.store_state_db(state.into());
    {
        let mut mem_pool = chain.mem_pool().await;
        mem_pool.push_transaction(batch_create_tx).unwrap();
    }

    let state = mem_pool_state.load_state_db();

    for wallet in new_users_wallet {
        let opt_user_script_hash = state
            .get_script_hash_by_registry_address(wallet.reg_address())
            .unwrap();

        assert_eq!(opt_user_script_hash, Some(wallet.account_script_hash()));
    }
}
