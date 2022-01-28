use anyhow::Result;
use ckb_crypto::secp::Privkey;
use ckb_types::prelude::{Builder, Entity};
use gw_common::{state::State, H256};
use gw_eoa_mapping::eth_register::EthEoaMappingRegister;
use gw_generator::{constants::L2TX_MAX_CYCLES, traits::StateExt};
use gw_store::{chain_view::ChainView, traits::chain_store::ChainStore};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{
        BlockInfo, ETHAddrRegArgs, ETHAddrRegArgsUnion, EthToGw, GwToEth, RawL2Transaction, Script,
    },
    prelude::{Pack, Unpack},
};
use gw_utils::wallet::{privkey_to_eth_account_script, Wallet};
use secp256k1::{rand::rngs::OsRng, Secp256k1};

use crate::testing_tool::chain::{
    setup_chain, ETH_ACCOUNT_LOCK_CODE_HASH, ETH_EOA_MAPPING_REGISTRY_VALIDATOR_CODE_HASH,
};

#[tokio::test]
async fn test_eth_eoa_mapping_register() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let rollup_type_script = Script::default();
    let rollup_script_hash: H256 = rollup_type_script.hash().into();
    let chain = setup_chain(rollup_type_script.clone()).await;
    let mem_pool_state = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        mem_pool.lock().await.mem_pool_state()
    };
    let snap = mem_pool_state.load();
    let mut state = snap.state()?;

    let secp = Secp256k1::new();
    let mut rng = OsRng::new().expect("OsRng");

    let eth_mapping_script = Script::new_builder()
        .code_hash((*ETH_EOA_MAPPING_REGISTRY_VALIDATOR_CODE_HASH).pack())
        .hash_type(ScriptHashType::Type.into())
        .args(Bytes::copy_from_slice(rollup_script_hash.as_slice()).pack())
        .build();
    state.create_account_from_script(eth_mapping_script)?;

    let random_eth_account_script = |rng: &mut _| -> Script {
        let sk = {
            let (sk, _public_key) = secp.generate_keypair(rng);
            Privkey::from_slice(&sk.serialize_secret())
        };
        privkey_to_eth_account_script(
            &sk,
            &rollup_script_hash,
            &(*ETH_ACCOUNT_LOCK_CODE_HASH).into(),
        )
        .unwrap()
    };

    let register_privkey = {
        let (sk, _public_key) = secp.generate_keypair(&mut rng);
        Privkey::from_slice(&sk.serialize_secret())
    };
    let register_account_script = privkey_to_eth_account_script(
        &register_privkey,
        &rollup_script_hash,
        &(*ETH_ACCOUNT_LOCK_CODE_HASH).into(),
    )?;
    let register_account_id = state.create_account_from_script(register_account_script.clone())?;
    let register_wallet = Wallet::new(register_privkey, register_account_script);

    let mapping_register = EthEoaMappingRegister::create(
        &state,
        rollup_script_hash,
        (*ETH_EOA_MAPPING_REGISTRY_VALIDATOR_CODE_HASH).into(),
        (*ETH_ACCOUNT_LOCK_CODE_HASH).into(),
        register_wallet,
    )?;

    let accounts_count = 5;
    let account_scripts: Vec<_> = (0..accounts_count)
        .map(|_| random_eth_account_script(&mut rng))
        .collect();
    let account_hashes: Vec<_> = account_scripts.iter().map(|s| s.hash().into()).collect();
    let account_eth_addrs = account_scripts.iter().map(|s| {
        let mut buf = [0u8; 20];
        buf.copy_from_slice(&Unpack::<Bytes>::unpack(&s.args())[32..]);
        buf
    });

    let from_id = state.get_account_count()?;
    for script in account_scripts.clone() {
        state.create_account_from_script(script.to_owned())?;
    }
    let script_hashes = mapping_register.filter_accounts(&state, from_id, from_id + 10)?;
    assert_eq!(script_hashes, account_hashes);

    let tx = mapping_register.build_register_tx(&state, script_hashes)?;

    {
        let mut mem_pool = chain.mem_pool().as_ref().unwrap().lock().await;
        mem_pool.push_transaction(tx).await?;
    }

    // Verify mapping register
    let tip_block_hash = chain.store().get_tip_block_hash()?;
    let db = chain.store().begin_transaction();
    let block_info = BlockInfo::new_builder()
        .block_producer_id(register_account_id.pack())
        .number(1.pack())
        .build();
    let chain_view = ChainView::new(&db, tip_block_hash);
    let generator = chain.generator();
    for (eth_eoa_address, eth_eoa_account_script_hash) in
        account_eth_addrs.into_iter().zip(account_hashes)
    {
        let eth_eoa_account_script_hash: [u8; 32] = eth_eoa_account_script_hash.into();

        // check result: eth_address -> gw_script_hash
        let args = {
            let to = EthToGw::new_builder()
                .eth_address(eth_eoa_address.pack())
                .build();
            ETHAddrRegArgs::new_builder()
                .set(ETHAddrRegArgsUnion::EthToGw(to))
                .build()
        };
        let raw_l2tx = RawL2Transaction::new_builder()
            .from_id(register_account_id.pack())
            .to_id(mapping_register.registry_account_id().pack())
            .args(args.as_bytes().pack())
            .build();
        let run_result = generator
            .execute_transaction(
                &chain_view,
                &state,
                &block_info,
                &raw_l2tx,
                L2TX_MAX_CYCLES,
                None,
            )
            .expect("execute eth addr registry contract");
        assert_eq!(run_result.return_data, eth_eoa_account_script_hash);

        // check result: gw_script_hash -> eth_address
        let args = {
            let to = GwToEth::new_builder()
                .gw_script_hash(eth_eoa_account_script_hash.pack())
                .build();
            ETHAddrRegArgs::new_builder()
                .set(ETHAddrRegArgsUnion::GwToEth(to))
                .build()
        };
        let raw_l2tx = RawL2Transaction::new_builder()
            .from_id(register_account_id.pack())
            .to_id(mapping_register.registry_account_id().pack())
            .args(args.as_bytes().pack())
            .build();
        let run_result = generator
            .execute_transaction(
                &chain_view,
                &state,
                &block_info,
                &raw_l2tx,
                L2TX_MAX_CYCLES,
                None,
            )
            .expect("execute eth addr registry contract");
        assert_eq!(run_result.return_data, eth_eoa_address);
    }

    Ok(())
}
