use std::{collections::HashMap, convert::TryInto, path::PathBuf, sync::Arc, time::Instant};

use anyhow::{anyhow, Context, Result};
use async_jsonrpc_client::HttpClient;
use ckb_types::{bytes::Bytes, prelude::Entity};
use gw_chain::chain::Chain;
use gw_common::H256;
use gw_config::{ChainConfig, Config, StoreConfig};
use gw_db::{schema::COLUMNS, RocksDB};
use gw_generator::{
    account_lock_manage::{
        secp256k1::{Secp256k1Eth, Secp256k1Tron},
        AccountLockManage,
    },
    backend_manage::BackendManage,
    genesis::init_genesis,
    Generator,
};
use gw_rpc_client::rpc_client::RPCClient;
use gw_store::{transaction::StoreTransaction, Store};
use gw_types::{
    core::ChallengeTargetType,
    offchain::RollupContext,
    packed::{Byte32, L2Block, RollupConfig},
    prelude::{Pack, Unpack},
};

pub struct ReplayArgs {
    pub from_db_store: PathBuf,
    pub from_db_columns: u32,
    pub to_db_store: PathBuf,
    pub config: Config,
}

pub fn replay(args: ReplayArgs) -> Result<()> {
    let ReplayArgs {
        from_db_store,
        to_db_store,
        config,
        from_db_columns,
    } = args;

    let store_config = StoreConfig {
        path: to_db_store,
        options: config.store.options.clone(),
        options_file: config.store.options_file.clone(),
        cache_size: config.store.cache_size.clone(),
    };
    let local_store = Store::new(RocksDB::open(&store_config, COLUMNS));
    let rollup_type_script = {
        let script: gw_types::packed::Script = config.chain.rollup_type_script.clone().into();
        script
    };
    let rollup_config: RollupConfig = config.genesis.rollup_config.clone().into();
    let rollup_context = RollupContext {
        rollup_config: rollup_config.clone(),
        rollup_script_hash: {
            let rollup_script_hash: [u8; 32] = config.genesis.rollup_type_hash.clone().into();
            rollup_script_hash.into()
        },
    };
    let secp_data: Bytes = {
        let rpc_client = {
            let indexer_client = HttpClient::new(config.rpc_client.indexer_url.to_owned())?;
            let ckb_client = HttpClient::new(config.rpc_client.ckb_url.to_owned())?;
            let rollup_type_script =
                ckb_types::packed::Script::new_unchecked(rollup_type_script.as_bytes());
            RPCClient::new(
                rollup_type_script,
                rollup_context.clone(),
                ckb_client,
                indexer_client,
            )
        };
        let out_point = config.genesis.secp_data_dep.out_point.clone();
        smol::block_on(rpc_client.get_transaction(out_point.tx_hash.0.into()))?
            .ok_or_else(|| anyhow!("can not found transaction: {:?}", out_point.tx_hash))?
            .raw()
            .outputs_data()
            .get(out_point.index.value() as usize)
            .expect("get secp output data")
            .raw_data()
    };

    init_genesis(
        &local_store,
        &config.genesis,
        config.chain.genesis_committed_info.clone().into(),
        secp_data.clone(),
    )
    .with_context(|| "init genesis")?;
    let generator = {
        let backend_manage = BackendManage::from_config(config.backends.clone())
            .with_context(|| "config backends")?;
        let mut account_lock_manage = AccountLockManage::default();
        let eth_lock_script_type_hash = rollup_config
            .allowed_eoa_type_hashes()
            .get(0)
            .ok_or_else(|| anyhow!("Eth: No allowed EoA type hashes in the rollup config"))?;
        account_lock_manage.register_lock_algorithm(
            eth_lock_script_type_hash.unpack(),
            Box::new(Secp256k1Eth::default()),
        );
        let tron_lock_script_type_hash = rollup_config.allowed_eoa_type_hashes().get(1);
        if let Some(code_hash) = tron_lock_script_type_hash {
            account_lock_manage
                .register_lock_algorithm(code_hash.unpack(), Box::new(Secp256k1Tron::default()))
        }
        Arc::new(Generator::new(
            backend_manage,
            account_lock_manage,
            rollup_context.clone(),
            Some(config.rpc.clone()),
        ))
    };

    let mut chain = Chain::create(
        &rollup_config,
        &rollup_type_script,
        &config.chain,
        local_store.clone(),
        generator,
        None,
    )?;

    let from_store = {
        let store_config = StoreConfig {
            path: from_db_store,
            options: config.store.options.clone(),
            options_file: config.store.options_file.clone(),
            cache_size: config.store.cache_size.clone(),
        };
        Store::new(RocksDB::open(&store_config, from_db_columns))
    };

    println!(
        "Skip blocks: {:?}",
        config
            .chain
            .skipped_invalid_block_list
            .iter()
            .map(|hash| hash.to_string())
            .collect::<Vec<_>>()
    );

    replay_chain(&mut chain, from_store, local_store)?;

    Ok(())
}

pub fn replay_chain(chain: &mut Chain, from_store: Store, local_store: Store) -> Result<()> {
    let tip = local_store.get_tip_block()?;
    let number = {
        let block_hash = from_store.get_block_hash_by_number(tip.raw().number().unpack())?;
        assert_eq!(H256::from(tip.hash()), block_hash.unwrap());
        tip.raw().number().unpack()
    };

    let hash: Byte32 = tip.hash().pack();
    println!("Replay from block: #{} {}", number, hash);

    // query next block
    let mut replay_number = number + 1;

    loop {
        let now = Instant::now();
        let block_hash = match from_store.get_block_hash_by_number(replay_number)? {
            Some(block_hash) => block_hash,
            None => {
                println!("Can't find block #{}, stop replay", replay_number);
                break;
            }
        };

        let block = from_store.get_block(&block_hash)?.expect("block");
        let block_number: u64 = block.raw().number().unpack();
        assert_eq!(block_number, replay_number, "number should be consist");
        let block_committed_info = from_store
            .get_l2block_committed_info(&block_hash)?
            .expect("block");
        let global_state = from_store
            .get_block_post_global_state(&block.raw().parent_block_hash().unpack())?
            .expect("block prev global state");
        let deposit_requests = from_store
            .get_block_deposit_requests(&block_hash)?
            .expect("block deposit requests");
        let load_block_ms = now.elapsed().as_millis();

        let txs_len = block.transactions().item_count();
        let deposits_len = deposit_requests.len();
        let db = local_store.begin_transaction();
        let now = Instant::now();
        if let Some(challenge) = chain.process_block(
            &db,
            block,
            block_committed_info,
            global_state,
            deposit_requests,
            Default::default(),
        )? {
            let target_type: u8 = challenge.target_type().into();
            let target_type: ChallengeTargetType = target_type.try_into().unwrap();
            let target_index: u32 = challenge.target_index().unpack();
            println!(
                "Challenge found type: {:?} index: {}",
                target_type, target_index
            );
            return Err(anyhow!("challenge found"));
        }

        let process_block_ms = now.elapsed().as_millis();

        let now = Instant::now();
        db.commit()?;
        let db_commit_ms = now.elapsed().as_millis();

        println!(
            "Replay block: #{} {} (txs: {} deposits: {} process time: {}ms commit time: {}ms load time: {}ms)",
            replay_number,
            {
                let hash: Byte32 = block_hash.pack();
                hash
            },
            txs_len,
            deposits_len,
            process_block_ms,
            db_commit_ms,
            load_block_ms
        );

        replay_number += 1;
    }
    Ok(())
}
