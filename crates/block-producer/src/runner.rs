use crate::{
    block_producer::BlockProducer, poller::ChainUpdater, rpc_client::RPCClient, types::ChainEvent,
    utils::CKBGenesisInfo,
};
use anyhow::{anyhow, Context, Result};
use async_jsonrpc_client::HttpClient;
use futures::{executor::block_on, select, FutureExt};
use gw_chain::chain::Chain;
use gw_common::{builtins::ETH_SYMBOL, H256};
use gw_config::Config;
use gw_db::{config::Config as DBConfig, schema::COLUMNS, RocksDB};
use gw_generator::{
    account_lock_manage::{secp256k1::Secp256k1Eth, AccountLockManage},
    backend_manage::BackendManage,
    genesis::init_genesis,
    Generator, RollupContext,
};
use gw_mem_pool::pool::MemPool;
use gw_rpc_server::{registry::Registry, server::start_jsonrpc_server};
use gw_store::Store;
use gw_types::{
    bytes::Bytes,
    packed::{NumberHash, RollupConfig, Script},
    prelude::*,
};
use gw_web3_indexer::Web3Indexer;
use parking_lot::Mutex;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    ConnectOptions,
};
use std::{
    net::{SocketAddr, ToSocketAddrs},
    process::exit,
    sync::Arc,
    time::Duration,
};

async fn poll_loop(
    rpc_client: RPCClient,
    chain_updater: ChainUpdater,
    block_producer: BlockProducer,
    poll_interval: Duration,
) -> Result<()> {
    struct Inner {
        chain_updater: ChainUpdater,
        block_producer: BlockProducer,
    }

    let inner = Arc::new(smol::lock::Mutex::new(Inner {
        chain_updater,
        block_producer,
    }));
    // get tip
    let (mut tip_number, mut tip_hash) = {
        let tip = rpc_client.get_tip().await?;
        let tip_number: u64 = tip.number().unpack();
        let tip_hash: H256 = tip.block_hash().unpack();
        (tip_number, tip_hash)
    };
    loop {
        if let Some(block) = rpc_client.get_block_by_number(tip_number + 1).await? {
            let raw_header = block.header().raw();
            let event = if raw_header.parent_hash().as_slice() == tip_hash.as_slice() {
                // received new layer1 block
                log::info!(
                    "received new layer1 block {}, {}",
                    tip_number,
                    hex::encode(tip_hash.as_slice())
                );
                ChainEvent::NewBlock {
                    block: block.clone(),
                }
            } else {
                // layer1 reverted
                log::info!(
                    "layer1 reverted {}, {:?}",
                    tip_number,
                    hex::encode(tip_hash.as_slice())
                );
                ChainEvent::Reverted {
                    old_tip: NumberHash::new_builder()
                        .number(tip_number.pack())
                        .block_hash(tip_hash.pack())
                        .build(),
                    new_block: block.clone(),
                }
            };
            // must execute chain update before block producer, otherwise we may run into an invalid chain state
            // smol::spawn({
            let event = event.clone();
            let inner = inner.clone();
            // async move {
            let mut inner = inner.lock().await;
            if let Err(err) = inner.chain_updater.handle_event(event.clone()).await {
                log::error!(
                    "Error occured when polling chain_updater, event: {:?}, error: {}",
                    event,
                    err
                );
            }
            if let Err(err) = inner.block_producer.handle_event(event.clone()).await {
                log::error!(
                    "Error occured when polling block_producer, event: {:?}, error: {}",
                    event,
                    err
                );
            }
            // }
            // })
            // .detach();
            // update tip
            tip_number = raw_header.number().unpack();
            tip_hash = block.header().hash().into();
        } else {
            log::debug!(
                "Not found layer1 block #{} sleep {}s then retry",
                tip_number + 1,
                poll_interval.as_secs()
            );
            async_std::task::sleep(poll_interval).await;
        }
    }
}

pub fn run(config: Config) -> Result<()> {
    let rollup_config: RollupConfig = config.genesis.rollup_config.clone().into();
    let block_producer_config = config
        .block_producer
        .clone()
        .ok_or_else(|| anyhow!("not set block producer"))?;

    let rollup_context = RollupContext {
        rollup_config: rollup_config.clone(),
        rollup_script_hash: {
            let rollup_script_hash: [u8; 32] = config.genesis.rollup_type_hash.clone().into();
            rollup_script_hash.into()
        },
    };
    let rollup_type_script: Script = config.chain.rollup_type_script.clone().into();
    let rpc_client = {
        let indexer_client = HttpClient::new(config.rpc_client.indexer_url)?;
        let ckb_client = HttpClient::new(config.rpc_client.ckb_url)?;
        let rollup_type_script =
            ckb_types::packed::Script::new_unchecked(rollup_type_script.as_bytes());
        RPCClient {
            indexer_client,
            ckb_client,
            rollup_context: rollup_context.clone(),
            rollup_type_script,
        }
    };

    // Open store
    let store = if config.store.path.as_os_str().is_empty() {
        log::warn!("config.store.path is blank, using temporary store");
        Store::open_tmp().with_context(|| "init store")?
    } else {
        let db_config = DBConfig {
            path: config.store.path,
            options: Default::default(),
            options_file: Default::default(),
        };
        Store::new(RocksDB::open(&db_config, COLUMNS))
    };
    let secp_data: Bytes = {
        let out_point = config.genesis.secp_data_dep.out_point.clone();
        block_on(rpc_client.get_transaction(out_point.tx_hash.0.into()))?
            .ok_or_else(|| anyhow!("can not found transaction: {:?}", out_point.tx_hash))?
            .raw()
            .outputs_data()
            .get(out_point.index.value() as usize)
            .expect("get secp output data")
            .raw_data()
    };
    init_genesis(
        &store,
        &config.genesis,
        config.chain.genesis_committed_info.clone().into(),
        secp_data,
    )
    .with_context(|| "init genesis")?;

    let rollup_config_hash = rollup_config.hash().into();
    let generator = {
        let backend_manage = BackendManage::from_config(config.backends.clone())
            .with_context(|| "config backends")?;
        let mut account_lock_manage = AccountLockManage::default();
        let eth_lock_script = rollup_config
            .allowed_eoa_scripts()
            .into_iter()
            .find(|s| {
                let symbol: [u8; 8] = s.symbol().unpack();
                &symbol == ETH_SYMBOL
            })
            .ok_or_else(|| anyhow!("No allowed EoA type hashes in the rollup config"))?;
        account_lock_manage.register_lock_algorithm(
            eth_lock_script.type_hash().unpack(),
            Box::new(Secp256k1Eth::default()),
        );
        Arc::new(Generator::new(
            backend_manage,
            account_lock_manage,
            rollup_context.clone(),
        ))
    };
    let mem_pool = Arc::new(Mutex::new(
        MemPool::create(store.clone(), generator.clone()).with_context(|| "create mem-pool")?,
    ));
    let chain = Arc::new(Mutex::new(
        Chain::create(
            &rollup_config,
            &config.chain.rollup_type_script.clone().into(),
            store.clone(),
            generator.clone(),
            mem_pool.clone(),
        )
        .with_context(|| "create chain")?,
    ));

    // RPC registry
    let rpc_registry = Registry::new(store.clone(), mem_pool.clone(), generator.clone());

    // create web3 indexer
    let web3_indexer = match config.web3_indexer {
        Some(web3_indexer_config) => {
            let pool = smol::block_on(async {
                let mut opts: PgConnectOptions = web3_indexer_config.database_url.parse()?;
                opts.log_statements(log::LevelFilter::Debug)
                    .log_slow_statements(log::LevelFilter::Warn, Duration::from_secs(5));
                PgPoolOptions::new()
                    .max_connections(5)
                    .connect_with(opts)
                    .await
            })?;
            let polyjuce_type_script_hash = web3_indexer_config.polyjuice_script_type_hash;
            let eth_account_lock_hash = web3_indexer_config.eth_account_lock_hash;
            let web3_indexer = Web3Indexer::new(
                pool,
                config
                    .genesis
                    .rollup_config
                    .l2_sudt_validator_script_type_hash,
                polyjuce_type_script_hash,
                config.genesis.rollup_type_hash,
                eth_account_lock_hash,
                config.genesis.rollup_config.compatible_chain_id.into(),
            );
            Some(web3_indexer)
        }
        None => None,
    };
    // create chain updater
    let chain_updater = ChainUpdater::new(
        Arc::clone(&chain),
        rpc_client.clone(),
        rollup_context,
        rollup_type_script.clone(),
        web3_indexer,
    );

    let ckb_genesis_info = {
        let ckb_genesis = smol::block_on(async { rpc_client.get_block_by_number(0).await })?
            .ok_or_else(|| anyhow!("can't found CKB genesis block"))?;
        CKBGenesisInfo::from_block(&ckb_genesis)?
    };

    // create block producer
    let block_producer = BlockProducer::create(
        rollup_config_hash,
        store,
        generator,
        chain,
        mem_pool,
        rpc_client.clone(),
        ckb_genesis_info,
        block_producer_config,
    )
    .with_context(|| "init block producer")?;

    let (s, ctrl_c) = async_channel::bounded(100);
    let handle = move || {
        s.try_send(()).ok();
    };
    ctrlc::set_handler(handle).unwrap();

    let rpc_address: SocketAddr = {
        let mut addrs: Vec<_> = config.rpc_server.listen.to_socket_addrs()?.collect();
        if addrs.len() != 1 {
            return Err(anyhow!(
                "Invalid RPC listen address `{}`",
                &config.rpc_server.listen
            ));
        }
        addrs.remove(0)
    };

    {
        let rollup_type_script_hash = {
            let hash = rollup_type_script.hash();
            ckb_fixed_hash::H256::from_slice(&hash).unwrap()
        };
        let rollup_config_hash =
            ckb_fixed_hash::H256::from_slice(&rollup_config_hash.as_slice()).unwrap();
        log::info!("Rollup type script hash: {}", rollup_type_script_hash);
        log::info!("Rollup config hash: {}", rollup_config_hash);
    }

    smol::block_on(async {
        select! {
            _ = ctrl_c.recv().fuse() => log::info!("Exiting..."),
            e = poll_loop(rpc_client, chain_updater, block_producer, Duration::from_secs(3)).fuse() => {
                log::error!("Error in main poll loop: {:?}", e);
            }
            e = start_jsonrpc_server(rpc_address, rpc_registry).fuse() => {
                log::error!("Error running JSONRPC server: {:?}", e);
                exit(1);
            },
        };
    });

    Ok(())
}
