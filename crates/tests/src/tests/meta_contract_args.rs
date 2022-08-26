use ckb_types::prelude::{Builder, Entity};
use gw_common::{
    builtins::{ETH_REGISTRY_ACCOUNT_ID, RESERVED_ACCOUNT_ID},
    state::State,
};
use gw_generator::account_lock_manage::secp256k1::Secp256k1Eth;
use gw_types::{
    packed::{
        CreateAccount, DeprecatedMetaContractArgs, Fee, L2Transaction, RawL2Transaction, Script,
    },
    prelude::Pack,
};

use crate::testing_tool::{chain::TestChain, eth_wallet::EthWallet};

const META_CONTRACT_ACCOUNT_ID: u32 = RESERVED_ACCOUNT_ID;

#[tokio::test(flavor = "multi_thread")]
async fn test_backward_compatibility() {
    let _ = env_logger::builder().is_test(true).try_init();

    let rollup_type_script = Script::default();
    let chain = TestChain::setup(rollup_type_script).await;

    // Deploy erc20 contract for test
    let mem_pool_state = chain.mem_pool_state().await;
    let snap = mem_pool_state.load();
    let mut state = snap.state().unwrap();

    let register = EthWallet::random(chain.rollup_type_hash());
    let register_id = register
        .create_account(&mut state, 9000000u128.into())
        .unwrap();

    let new_user = EthWallet::random(chain.rollup_type_hash());

    let opt_user_id = state
        .get_account_id_by_script_hash(&new_user.account_script_hash())
        .unwrap();
    assert!(opt_user_id.is_none());

    let meta_contract_script_hash = state.get_script_hash(META_CONTRACT_ACCOUNT_ID).unwrap();
    let fee = Fee::new_builder()
        .registry_id(ETH_REGISTRY_ACCOUNT_ID.pack())
        .amount(0u128.pack())
        .build();
    let create_user = CreateAccount::new_builder()
        .fee(fee)
        .script(new_user.account_script().to_owned())
        .build();
    let args = DeprecatedMetaContractArgs::new_builder()
        .set(create_user)
        .build();

    let raw_l2tx = RawL2Transaction::new_builder()
        .chain_id(chain.chain_id().pack())
        .from_id(register_id.pack())
        .to_id(META_CONTRACT_ACCOUNT_ID.pack())
        .nonce(0u32.pack())
        .args(args.as_bytes().pack())
        .build();

    let signing_message = Secp256k1Eth::eip712_signing_message(
        chain.chain_id(),
        &raw_l2tx,
        register.reg_address().to_owned(),
        meta_contract_script_hash,
    )
    .unwrap();
    let sign = register.sign_message(signing_message.into()).unwrap();

    let create_user_tx = L2Transaction::new_builder()
        .raw(raw_l2tx)
        .signature(sign.pack())
        .build();

    mem_pool_state.store(snap.into());
    {
        let mut mem_pool = chain.mem_pool().await;
        mem_pool.push_transaction(create_user_tx).unwrap();
    }

    let snap = mem_pool_state.load();
    let state = snap.state().unwrap();

    let opt_user_id = state
        .get_account_id_by_script_hash(&new_user.account_script_hash())
        .unwrap();
    assert!(opt_user_id.is_some());
}
