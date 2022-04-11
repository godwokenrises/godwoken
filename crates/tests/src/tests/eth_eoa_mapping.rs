use anyhow::{anyhow, Result};
use ckb_crypto::secp::Privkey;
use ckb_types::prelude::{Builder, Entity};
use gw_chain::chain::{L1Action, L1ActionContext, SyncParam};
use gw_common::{state::State, H256};
use gw_eoa_mapping::eth_register::EthEoaMappingRegister;
use gw_generator::{constants::L2TX_MAX_CYCLES, traits::StateExt};
use gw_store::{chain_view::ChainView, traits::chain_store::ChainStore};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{
        BlockInfo, CellOutput, CreateAccount, DepositRequest, ETHAddrRegArgs, ETHAddrRegArgsUnion,
        EthToGw, Fee, GwToEth, L2BlockCommittedInfo, L2Transaction, MetaContractArgs,
        RawL2Transaction, Script,
    },
    prelude::{Pack, Unpack},
};
use gw_utils::wallet::{privkey_to_eth_account_script, Wallet};
use secp256k1::{rand::rngs::OsRng, Secp256k1};
use sha3::{Digest, Keccak256};

use crate::testing_tool::{
    chain::{
        build_sync_tx, construct_block, setup_chain, ETH_ACCOUNT_LOCK_CODE_HASH,
        ETH_EOA_MAPPING_REGISTRY_VALIDATOR_CODE_HASH,
    },
    common::random_always_success_script,
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
    let registry_account_id = state
        .get_account_id_by_script_hash(&mapping_register.registry_script_hash())?
        .ok_or_else(|| anyhow!("eth registry(contract) account not found"))?;
    let tip_block_hash = chain.store().get_tip_block_hash()?;
    let db = chain.store().begin_transaction();
    let block_info = BlockInfo::new_builder()
        .block_producer(Default::default())
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
            .to_id(registry_account_id.pack())
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
            .to_id(registry_account_id.pack())
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

const CKB: u64 = 100000000;
const DEPOSIT_CAPACITY: u64 = 1000000 * CKB;

#[tokio::test]
async fn test_mem_pool_eth_eoa_mapping_deposit_scan_and_register() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let rollup_type_script = Script::default();
    let rollup_script_hash: H256 = rollup_type_script.hash().into();
    let rollup_cell = CellOutput::new_builder()
        .type_(Some(rollup_type_script.clone()).pack())
        .build();
    let mut chain = setup_chain(rollup_type_script.clone()).await;

    // Deposit eth mapping register
    let secp = Secp256k1::new();
    let mut rng = OsRng::new().expect("OsRng");

    let register_privkey = {
        let (sk, _public_key) = secp.generate_keypair(&mut rng);
        Privkey::from_slice(&sk.serialize_secret())
    };
    let register_account_script = privkey_to_eth_account_script(
        &register_privkey,
        &rollup_script_hash,
        &(*ETH_ACCOUNT_LOCK_CODE_HASH).into(),
    )?;
    let register_wallet = Wallet::new(register_privkey, register_account_script.clone());
    let register_eth_address = {
        let mut buf = [0u8; 20];
        buf.copy_from_slice(&Unpack::<Bytes>::unpack(&register_account_script.args())[32..]);
        buf
    };

    let deposits = vec![DepositRequest::new_builder()
        .capacity(DEPOSIT_CAPACITY.pack())
        .script(register_account_script.clone())
        .build()];

    let deposit_block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(&chain, &mut mem_pool, deposits.clone()).await?
    };
    let apply_deposits = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block: deposit_block_result.block.clone(),
            deposit_requests: deposits,
            deposit_asset_scripts: Default::default(),
            withdrawals: Default::default(),
        },
        transaction: build_sync_tx(rollup_cell.clone(), deposit_block_result.clone()),
        l2block_committed_info: L2BlockCommittedInfo::new_builder()
            .number(1u64.pack())
            .build(),
    };
    let param = SyncParam {
        updates: vec![apply_deposits],
        reverts: Default::default(),
    };
    chain.sync(param).await?;
    assert!(chain.last_sync_event().is_success());

    // Deploy eth eoa mapping registry contract
    let mem_pool_state = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        mem_pool.lock().await.mem_pool_state()
    };
    let snap = mem_pool_state.load();
    let state = snap.state()?;
    let register_account_id = state
        .get_account_id_by_script_hash(&register_account_script.hash().into())?
        .expect("register account id");

    let eth_mapping_script = Script::new_builder()
        .code_hash((*ETH_EOA_MAPPING_REGISTRY_VALIDATOR_CODE_HASH).pack())
        .hash_type(ScriptHashType::Type.into())
        .args(Bytes::copy_from_slice(rollup_script_hash.as_slice()).pack())
        .build();
    let meta_create_args = {
        let fee = 0u64;
        let create_account = CreateAccount::new_builder()
            .script(eth_mapping_script.clone())
            .fee(
                Fee::new_builder()
                    .amount(fee.pack())
                    .registry_id(gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID.pack())
                    .build(),
            )
            .build();
        MetaContractArgs::new_builder().set(create_account).build()
    };
    let nonce = state.get_nonce(register_account_id)?;
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(register_account_id.pack())
        .to_id(0u32.pack()) // 0 is reversed meta contract account id
        .nonce(nonce.pack())
        .args(meta_create_args.as_bytes().pack())
        .build();
    let register_account_script_hash = state.get_script_hash(register_account_id)?;
    assert_eq!(
        register_account_script_hash,
        register_account_script.hash().into()
    );

    let meta_account_script_hash = state.get_script_hash(0)?;
    let message = raw_tx.calc_message(
        &rollup_script_hash,
        &register_account_script_hash,
        &meta_account_script_hash,
    );
    let signing_message = {
        let mut hasher = Keccak256::new();
        hasher.update("\x19Ethereum Signed Message:\n32");
        hasher.update(message.as_slice());
        let buf = hasher.finalize();
        let mut signing_message = [0u8; 32];
        signing_message.copy_from_slice(&buf[..]);
        signing_message
    };
    let signature = register_wallet.sign_message(signing_message)?;
    let tx = L2Transaction::new_builder()
        .raw(raw_tx)
        .signature(signature.pack())
        .build();
    let deploy_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        mem_pool.push_transaction(tx).await?;
        construct_block(&chain, &mut mem_pool, vec![]).await?
    };
    let apply_deploy = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block: deploy_result.block.clone(),
            deposit_requests: vec![],
            deposit_asset_scripts: Default::default(),
            withdrawals: Default::default(),
        },
        transaction: build_sync_tx(rollup_cell.clone(), deploy_result),
        l2block_committed_info: L2BlockCommittedInfo::new_builder()
            .number(1u64.pack())
            .build(),
    };
    let param = SyncParam {
        updates: vec![apply_deploy],
        reverts: Default::default(),
    };
    chain.sync(param).await?;
    assert!(chain.last_sync_event().is_success());

    // Deploy random accounts
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

    let mapping_register = EthEoaMappingRegister::create(
        rollup_script_hash,
        (*ETH_EOA_MAPPING_REGISTRY_VALIDATOR_CODE_HASH).into(),
        (*ETH_ACCOUNT_LOCK_CODE_HASH).into(),
        register_wallet,
    )?;
    let registry_account_id = state
        .get_account_id_by_script_hash(&mapping_register.registry_script_hash())?
        .ok_or_else(|| anyhow!("eth registry(contract) account not found"))?;

    {
        let mut mem_pool = chain.mem_pool().as_ref().unwrap().lock().await;
        mem_pool.set_eth_eoa_mapping_register(mapping_register);
    }

    let accounts_count = 5;
    let eth_accounts_count = accounts_count / 2;
    let eth_account_scripts: Vec<_> = (0..eth_accounts_count)
        .map(|_| random_eth_account_script(&mut rng))
        .collect();
    let eth_account_hashes: Vec<H256> = { eth_account_scripts.iter() }
        .map(|s| s.hash().into())
        .collect();
    let eth_account_addrs = eth_account_scripts.iter().map(|s| {
        let mut buf = [0u8; 20];
        buf.copy_from_slice(&Unpack::<Bytes>::unpack(&s.args())[32..]);
        buf
    });
    let always_account_scripts: Vec<_> = (0..accounts_count - eth_accounts_count)
        .map(|_| random_always_success_script(&rollup_script_hash))
        .collect();

    // Deposit accounts
    let accounts: Vec<_> = { eth_account_scripts.clone().into_iter() }
        .chain(always_account_scripts)
        .collect();
    let deposits = accounts.iter().map(|account_script| {
        DepositRequest::new_builder()
            .capacity(DEPOSIT_CAPACITY.pack())
            .script(account_script.to_owned())
            .build()
    });

    let deposit_block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(&chain, &mut mem_pool, deposits.clone().collect()).await?
    };
    let apply_deposits = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block: deposit_block_result.block.clone(),
            deposit_requests: deposits.collect(),
            deposit_asset_scripts: Default::default(),
            withdrawals: Default::default(),
        },
        transaction: build_sync_tx(rollup_cell, deposit_block_result.clone()),
        l2block_committed_info: L2BlockCommittedInfo::new_builder()
            .number(1u64.pack())
            .build(),
    };
    let param = SyncParam {
        updates: vec![apply_deposits],
        reverts: Default::default(),
    };
    chain.sync(param).await?;
    assert!(chain.last_sync_event().is_success());

    // Verify mapping register
    let tip_block_hash = chain.store().get_tip_block_hash()?;
    let db = chain.store().begin_transaction();
    let block_info = BlockInfo::new_builder()
        .block_producer(Default::default())
        .number(4.pack())
        .build();
    let chain_view = ChainView::new(&db, tip_block_hash);
    let generator = chain.generator();
    let mem_pool_state = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        mem_pool.lock().await.mem_pool_state()
    };
    let snap = mem_pool_state.load();
    let state = snap.state()?;
    // Also check register, it should be register too
    for (eth_eoa_address, eth_eoa_account_script_hash) in eth_account_addrs
        .into_iter()
        .zip(eth_account_hashes)
        .chain([(register_eth_address, register_account_script_hash)])
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
            .to_id(registry_account_id.pack())
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
            .to_id(registry_account_id.pack())
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
