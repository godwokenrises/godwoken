#![allow(clippy::mutable_key_type)]

use anyhow::Result;
use ckb_types::prelude::{Builder, Entity};
use gw_common::{builtins::CKB_SUDT_ACCOUNT_ID, state::State, H256};
use gw_rpc_server::polyjuice_tx::eoa_creator::PolyjuiceEthEoaCreator;
use gw_types::{
    packed::{RawL2Transaction, Script},
    prelude::{Pack, Unpack},
};

use crate::testing_tool::{
    chain::{setup_chain, ETH_ACCOUNT_LOCK_CODE_HASH},
    eth_wallet::EthWallet,
    polyjuice::{PolyjuiceAccount, PolyjuiceArgsBuilder},
};

#[tokio::test(flavor = "multi_thread")]
async fn test_eoa_creator() {
    let _ = env_logger::builder().is_test(true).try_init();

    let rollup_type_script = Script::default();
    let rollup_script_hash: H256 = rollup_type_script.hash().into();

    let chain = setup_chain(rollup_type_script.clone()).await;
    let chain_id = {
        let config = &chain.generator().rollup_context().rollup_config;
        config.chain_id().unpack()
    };

    let mem_pool_state = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        mem_pool.lock().await.mem_pool_state()
    };
    let snap = mem_pool_state.load();
    let mut state = snap.state().unwrap();

    let creator_wallet = EthWallet::random(rollup_script_hash);
    creator_wallet
        .create_account(&mut state, 10000u128.into())
        .unwrap();

    let polyjuice_account = PolyjuiceAccount::create(rollup_script_hash, &mut state).unwrap();
    state.submit_tree_to_mem_block();

    let eth_eoa_creator = PolyjuiceEthEoaCreator::create(
        &state,
        chain_id,
        rollup_script_hash,
        (*ETH_ACCOUNT_LOCK_CODE_HASH).into(),
        creator_wallet.inner,
    )
    .unwrap();

    let eth_eoa_count = 5;
    let eth_eoa_wallet: Vec<_> = (0..eth_eoa_count)
        .map(|_| EthWallet::random(rollup_script_hash))
        .collect();

    for wallet in eth_eoa_wallet.iter() {
        wallet
            .mint_sudt(&mut state, CKB_SUDT_ACCOUNT_ID, 1u128.into())
            .unwrap();
    }

    let polyjuice_create_args = PolyjuiceArgsBuilder::default()
        .create(true)
        .data(b"POLYJUICEcontract".to_vec())
        .finish();

    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain_id.pack())
        .from_id(0u32.pack())
        .to_id(polyjuice_account.id.pack())
        .nonce(0u32.pack())
        .args(polyjuice_create_args.pack())
        .build();

    let txs = eth_eoa_wallet
        .iter()
        .map(|wallet| wallet.sign_polyjuice_tx(&state, raw_tx.clone()))
        .collect::<Result<Vec<_>>>()
        .unwrap();

    let map_sig_eoa_scripts = eth_eoa_creator.filter_map_from_id_zero_has_ckb_balance(&state, &txs);
    assert_eq!(map_sig_eoa_scripts.len(), eth_eoa_wallet.len());

    let batch_create_tx = eth_eoa_creator
        .build_batch_create_tx(&state, map_sig_eoa_scripts.values())
        .unwrap();

    {
        let opt_mem_pool = chain.mem_pool().as_ref();
        let mut mem_pool = opt_mem_pool.unwrap().lock().await;
        mem_pool.push_transaction(batch_create_tx).await.unwrap();
    }

    for wallet in eth_eoa_wallet {
        let opt_eoa_account_script_hash = state
            .get_script_hash_by_registry_address(&wallet.registry_address)
            .unwrap();

        assert_eq!(
            opt_eoa_account_script_hash,
            Some(wallet.account_script().hash().into())
        );
    }
}
