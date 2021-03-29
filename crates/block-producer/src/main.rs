use anyhow::{anyhow, Result};
use async_jsonrpc_client::HttpClient;
use futures::{select, FutureExt};
use gw_block_producer::{
    block_producer::BlockProducer, poller::ChainUpdater, rpc_client::RPCClient,
};
use gw_chain::chain::Chain;
use gw_config::Config;
use gw_generator::{
    account_lock_manage::AccountLockManage, backend_manage::BackendManage, genesis::init_genesis,
    Generator, RollupContext,
};
use gw_mem_pool::pool::MemPool;
use gw_store::Store;
use gw_types::{packed::Script, prelude::*};
use parking_lot::Mutex;
use std::{fs, path::Path, process::exit, sync::Arc};

fn read_config<P: AsRef<Path>>(path: P) -> Result<Config> {
    let content = fs::read(path)?;
    let config = toml::from_slice(&content)?;
    Ok(config)
}

fn run() -> Result<()> {
    let config_path = "./config.toml";
    // read config
    let config = read_config(&config_path)?;
    // TODO: use persistent store later
    let store = Store::open_tmp()?;
    init_genesis(
        &store,
        &config.genesis,
        config.rollup_deployment.genesis_header.clone().into(),
    )?;
    let rollup_context = RollupContext {
        rollup_config: config.genesis.rollup_config.clone().into(),
        rollup_script_hash: {
            let rollup_script_hash: [u8; 32] = config.genesis.rollup_script_hash.clone().into();
            rollup_script_hash.into()
        },
    };

    let rollup_config_hash = rollup_context.rollup_config.hash().into();
    let generator = {
        let backend_manage = BackendManage::from_config(config.backends.clone())?;
        let account_lock_manage = AccountLockManage::default();
        Arc::new(Generator::new(
            backend_manage,
            account_lock_manage,
            rollup_context.clone(),
        ))
    };
    let mem_pool = Arc::new(Mutex::new(MemPool::create(
        store.clone(),
        generator.clone(),
    )?));
    let chain = Arc::new(Mutex::new(Chain::create(
        config.chain.clone(),
        store.clone(),
        generator.clone(),
        mem_pool.clone(),
    )?));

    let rollup_type_script: Script = config.chain.rollup_type_script.into();
    let rpc_client = {
        let indexer_client = HttpClient::new(config.rpc_client.indexer_url)?;
        let ckb_client = HttpClient::new(config.rpc_client.ckb_url)?;
        let rollup_type_script =
            { ckb_types::packed::Script::new_unchecked(rollup_type_script.as_bytes()) };
        RPCClient {
            indexer_client,
            ckb_client,
            rollup_context: rollup_context.clone(),
            rollup_type_script,
        }
    };

    // create chain updater
    let mut chain_updater = ChainUpdater::new(
        Arc::clone(&chain),
        rpc_client.clone(),
        rollup_context,
        rollup_type_script,
    );

    // create block producer
    let block_producer = BlockProducer::create(
        rollup_config_hash,
        store,
        generator,
        chain,
        mem_pool,
        rpc_client,
        config
            .block_producer
            .ok_or_else(|| anyhow!("not set block producer"))?,
    )?;

    let (s, ctrl_c) = async_channel::bounded(100);
    let handle = move || {
        s.try_send(()).ok();
    };
    ctrlc::set_handler(handle).unwrap();

    smol::block_on(async {
        select! {
            _ = ctrl_c.recv().fuse() => println!("Exiting..."),
            e = chain_updater.poll_loop().fuse() => {
                eprintln!("Error occurs polling blocks: {:?}", e);
                exit(1);
            },
            e = block_producer.poll_loop().fuse() => {
                eprintln!("Error occurs produce block: {:?}", e);
            }
            // e = start_jsonrpc_server(matches.value_of("listen").unwrap().to_string()).fuse() => {
            //     info!("Error running JSONRPC server: {:?}", e);
            //     exit(1);
            // },
        };
    });

    Ok(())
}

/// Block producer
fn main() {
    run().expect("block producer");
}
