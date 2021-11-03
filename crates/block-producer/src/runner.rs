use crate::{
    block_producer::BlockProducer, challenger::Challenger, cleaner::Cleaner, poller::ChainUpdater,
    test_mode_control::TestModeControl, types::ChainEvent,
};
use anyhow::{anyhow, Context, Result};
use async_jsonrpc_client::HttpClient;
use ckb_types::core::hardfork::HardForkSwitch;
use gw_chain::chain::Chain;
use gw_challenge::offchain::{OffChainMockContext, OffChainValidatorContext};
use gw_ckb_hardfork::{GLOBAL_CURRENT_EPOCH_NUMBER, GLOBAL_HARDFORK_SWITCH, GLOBAL_VM_VERSION};
use gw_common::{blake2b::new_blake2b, H256};
use gw_config::{BlockProducerConfig, Config, NodeMode};
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
use gw_mem_pool::{
    batch::MemPoolBatch, default_provider::DefaultMemPoolProvider, pool::MemPool,
    traits::MemPoolErrorTxHandler,
};
use gw_poa::PoA;
use gw_rpc_client::rpc_client::RPCClient;
use gw_rpc_server::{registry::Registry, server::start_jsonrpc_server};
use gw_store::Store;
use gw_types::{
    bytes::Bytes,
    offchain::RollupContext,
    packed::{CellDep, NumberHash, RollupConfig, Script},
    prelude::*,
};
use gw_utils::{genesis_info::CKBGenesisInfo, wallet::Wallet};
use gw_web3_indexer::{ErrorReceiptIndexer, Web3Indexer};
use semver::Version;
use smol::lock::Mutex;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    ConnectOptions,
};
use std::{
    collections::HashMap,
    net::{SocketAddr, ToSocketAddrs},
    sync::Arc,
    time::{Duration, Instant},
};

const MIN_CKB_VERSION: &str = "0.40.0";
const SMOL_THREADS_ENV_VAR: &str = "SMOL_THREADS";
const DEFAULT_RUNTIME_THREADS: usize = 4;

async fn poll_loop(
    rpc_client: RPCClient,
    chain_updater: ChainUpdater,
    block_producer: Option<BlockProducer>,
    challenger: Option<Challenger>,
    cleaner: Option<Arc<Cleaner>>,
    poll_interval: Duration,
) -> Result<()> {
    struct Inner {
        chain_updater: ChainUpdater,
        block_producer: Option<BlockProducer>,
        challenger: Option<Challenger>,
        cleaner: Option<Arc<Cleaner>>,
    }

    let inner = Arc::new(smol::lock::Mutex::new(Inner {
        chain_updater,
        challenger,
        block_producer,
        cleaner,
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
            let event = event.clone();
            let inner = inner.clone();
            let mut inner = inner.lock().await;
            inner
                .chain_updater
                .handle_event(event.clone())
                .await
                .map_err(|err| {
                    anyhow!(
                        "Error occured when polling chain_updater, event: {}, error: {}",
                        event,
                        err
                    )
                })?;

            if let Some(ref mut challenger) = inner.challenger {
                challenger
                    .handle_event(event.clone())
                    .await
                    .map_err(|err| {
                        anyhow!(
                            "Error occured when polling challenger, event: {}, error: {}",
                            event,
                            err
                        )
                    })?;
            }

            if let Some(ref mut block_producer) = inner.block_producer {
                block_producer
                    .handle_event(event.clone())
                    .await
                    .map_err(|err| {
                        anyhow!(
                            "Error occured when polling block_producer, event: {}, error: {}",
                            event,
                            err
                        )
                    })?;
            }

            if let Some(ref cleaner) = inner.cleaner {
                cleaner.handle_event(event.clone()).await.map_err(|err| {
                    anyhow!(
                        "Error occured when polling block_producer, event: {}, error: {}",
                        event,
                        err
                    )
                })?;
            }

            // update tip
            tip_number = raw_header.number().unpack();
            tip_hash = block.header().hash().into();

            // update global hardfork info
            let hardfork_switch = rpc_client.get_hardfork_switch().await?;
            let rpc32_epoch_number = hardfork_switch.rfc_0032();
            let mut global_hardfork_switch = GLOBAL_HARDFORK_SWITCH.lock().await;
            if !is_hardfork_switch_eq(&*global_hardfork_switch, &hardfork_switch) {
                *global_hardfork_switch = hardfork_switch
            }

            // update global current epoch number
            let current_epoch_number = rpc_client.get_current_epoch_number().await?;
            let mut global_epoch_number = GLOBAL_CURRENT_EPOCH_NUMBER.lock().await;
            if *global_epoch_number != current_epoch_number {
                *global_epoch_number = current_epoch_number;
            }

            // update global vm version
            let vm_version: u32 = if current_epoch_number >= rpc32_epoch_number {
                1
            } else {
                0
            };
            let mut global_vm_version = GLOBAL_VM_VERSION.lock().await;
            if *global_vm_version != vm_version {
                *global_vm_version = vm_version;
            }
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

pub struct BaseInitComponents {
    pub rollup_config: RollupConfig,
    pub rollup_config_hash: H256,
    pub rollup_context: RollupContext,
    pub rollup_type_script: Script,
    pub builtin_load_data: HashMap<H256, CellDep>,
    pub ckb_genesis_info: CKBGenesisInfo,
    pub rpc_client: RPCClient,
    pub store: Store,
    pub generator: Arc<Generator>,
}

impl BaseInitComponents {
    pub fn init(config: &Config, skip_config_check: bool) -> Result<Self> {
        let rollup_config: RollupConfig = config.genesis.rollup_config.clone().into();
        let rollup_context = RollupContext {
            rollup_config: rollup_config.clone(),
            rollup_script_hash: {
                let rollup_script_hash: [u8; 32] = config.genesis.rollup_type_hash.clone().into();
                rollup_script_hash.into()
            },
        };
        let rollup_type_script: Script = config.chain.rollup_type_script.clone().into();
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

        if !skip_config_check {
            check_ckb_version(&rpc_client)?;
            // TODO: check ckb indexer version
            if NodeMode::ReadOnly != config.node_mode {
                let block_producer_config = config
                    .block_producer
                    .clone()
                    .ok_or_else(|| anyhow!("not set block producer"))?;
                check_rollup_config_cell(&block_producer_config, &rollup_config, &rpc_client)?;
                check_locks(&block_producer_config, &rollup_config)?;
            }
        }

        // Open store
        let timer = Instant::now();
        let store = if config.store.path.as_os_str().is_empty() {
            log::warn!("config.store.path is blank, using temporary store");
            Store::open_tmp().with_context(|| "init store")?
        } else {
            Store::new(RocksDB::open(&config.store, COLUMNS))
        };
        let elapsed_ms = timer.elapsed().as_millis();
        log::debug!("Open rocksdb costs: {}ms.", elapsed_ms);

        let secp_data: Bytes = {
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
            &store,
            &config.genesis,
            config.chain.genesis_committed_info.clone().into(),
            secp_data.clone(),
        )
        .with_context(|| "init genesis")?;

        let rollup_config_hash: H256 = rollup_config.hash().into();
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
                config.rpc.clone(),
            ))
        };

        let ckb_genesis_info = {
            let ckb_genesis = smol::block_on(async { rpc_client.get_block_by_number(0).await })?
                .ok_or_else(|| anyhow!("can't found CKB genesis block"))?;
            CKBGenesisInfo::from_block(&ckb_genesis)?
        };

        let to_hash = |data| -> [u8; 32] {
            let mut hasher = new_blake2b();
            hasher.update(data);
            let mut hash = [0u8; 32];
            hasher.finalize(&mut hash);
            hash
        };
        let mut builtin_load_data = HashMap::new();
        builtin_load_data.insert(
            to_hash(secp_data.as_ref()).into(),
            config.genesis.secp_data_dep.clone().into(),
        );

        let base = BaseInitComponents {
            rollup_config,
            rollup_config_hash,
            rollup_context,
            rollup_type_script,
            builtin_load_data,
            ckb_genesis_info,
            rpc_client,
            store,
            generator,
        };

        Ok(base)
    }

    pub fn init_poa(
        &self,
        wallet: &Wallet,
        block_producer_config: &BlockProducerConfig,
    ) -> Arc<Mutex<PoA>> {
        let poa = PoA::new(
            self.rpc_client.clone(),
            wallet.lock_script().to_owned(),
            block_producer_config.poa_lock_dep.clone().into(),
            block_producer_config.poa_state_dep.clone().into(),
        );
        Arc::new(smol::lock::Mutex::new(poa))
    }

    pub async fn init_offchain_mock_context(
        &self,
        poa: &PoA,
        block_producer_config: &BlockProducerConfig,
    ) -> Result<OffChainMockContext> {
        let ckb_genesis_info = gw_challenge::offchain::CKBGenesisInfo {
            sighash_dep: self.ckb_genesis_info.sighash_dep(),
        };
        let wallet = {
            let config = &block_producer_config.wallet_config;
            Wallet::from_config(config).with_context(|| "init wallet")?
        };

        OffChainMockContext::build(
            &self.rpc_client,
            poa,
            self.rollup_context.clone(),
            wallet,
            block_producer_config.clone(),
            ckb_genesis_info,
            self.builtin_load_data.clone(),
        )
        .await
    }
}

pub fn run(config: Config, skip_config_check: bool) -> Result<()> {
    // Enable smol threads before smol::spawn
    let runtime_threads = match std::env::var(SMOL_THREADS_ENV_VAR) {
        Ok(s) => s.parse()?,
        Err(_) => {
            let threads = DEFAULT_RUNTIME_THREADS;
            std::env::set_var(SMOL_THREADS_ENV_VAR, format!("{}", threads));
            threads
        }
    };
    log::info!(
        "Runtime threads: {}. (set environment '{}' to tune this value)",
        runtime_threads,
        SMOL_THREADS_ENV_VAR
    );

    let base = BaseInitComponents::init(&config, skip_config_check)?;
    let (mem_pool, wallet, poa, offchain_mock_context, pg_pool) = match config
        .block_producer
        .clone()
    {
        Some(block_producer_config) => {
            let wallet = Wallet::from_config(&block_producer_config.wallet_config)
                .with_context(|| "init wallet")?;
            let poa = base.init_poa(&wallet, &block_producer_config);
            let offchain_mock_context = smol::block_on(async {
                let poa = poa.lock().await;
                base.init_offchain_mock_context(&poa, &block_producer_config)
                    .await
            })?;

            let mut offchain_validator_context = None;
            if let Some(validator_config) = config.offchain_validator {
                let debug_config = config.debug.clone();

                let context = OffChainValidatorContext::build(
                    &offchain_mock_context,
                    debug_config,
                    validator_config,
                )?;

                offchain_validator_context = Some(context);
            }

            let mem_pool_provider = DefaultMemPoolProvider::new(
                base.rpc_client.clone(),
                Arc::clone(&poa),
                base.store.clone(),
            );
            let pg_pool = {
                let config = config.web3_indexer.as_ref();
                let init_pool = config.map(|web3_indexer_config| {
                    smol::block_on(async {
                        let mut opts: PgConnectOptions =
                            web3_indexer_config.database_url.parse()?;
                        opts.log_statements(log::LevelFilter::Debug)
                            .log_slow_statements(log::LevelFilter::Warn, Duration::from_secs(5));
                        PgPoolOptions::new()
                            .max_connections(5)
                            .connect_with(opts)
                            .await
                    })
                });
                init_pool.transpose()?
            };
            let error_tx_handler = pg_pool.clone().map(|pool| {
                Box::new(ErrorReceiptIndexer::new(pool)) as Box<dyn MemPoolErrorTxHandler + Send>
            });
            let mem_pool = Arc::new(Mutex::new(
                MemPool::create(
                    base.store.clone(),
                    base.generator.clone(),
                    Box::new(mem_pool_provider),
                    error_tx_handler,
                    offchain_validator_context,
                    config.mem_pool.clone(),
                )
                .with_context(|| "create mem-pool")?,
            ));
            (
                Some(mem_pool),
                Some(wallet),
                Some(poa),
                Some(offchain_mock_context),
                pg_pool,
            )
        }
        None => (None, None, None, None, None),
    };

    let BaseInitComponents {
        rollup_config,
        rollup_config_hash,
        rollup_context,
        rollup_type_script,
        builtin_load_data,
        ckb_genesis_info,
        rpc_client,
        store,
        generator,
    } = base;

    let chain = Arc::new(Mutex::new(
        Chain::create(
            &rollup_config,
            &config.chain.rollup_type_script.clone().into(),
            &config.chain,
            store.clone(),
            generator.clone(),
            mem_pool.clone(),
        )
        .with_context(|| "create chain")?,
    ));

    // create web3 indexer
    let web3_indexer = match config.web3_indexer {
        Some(web3_indexer_config) => {
            let pool = pg_pool.unwrap();
            let polyjuce_type_script_hash = web3_indexer_config.polyjuice_script_type_hash;
            let eth_account_lock_hash = web3_indexer_config.eth_account_lock_hash;
            let tron_account_lock_hash = web3_indexer_config.tron_account_lock_hash;
            let web3_indexer = Web3Indexer::new(
                pool,
                config
                    .genesis
                    .rollup_config
                    .l2_sudt_validator_script_type_hash,
                polyjuce_type_script_hash,
                config.genesis.rollup_type_hash,
                eth_account_lock_hash,
                tron_account_lock_hash,
            );
            // fix missing genesis block
            smol::block_on(web3_indexer.store_genesis(store.clone()))?;
            Some(web3_indexer)
        }
        None => None,
    };

    // create chain updater
    let chain_updater = ChainUpdater::new(
        Arc::clone(&chain),
        rpc_client.clone(),
        rollup_context.clone(),
        rollup_type_script.clone(),
        web3_indexer,
    );

    let (block_producer, challenger, test_mode_control, cleaner) = match config.node_mode {
        NodeMode::ReadOnly => (None, None, None, None),
        mode => {
            let block_producer_config = config
                .block_producer
                .clone()
                .ok_or_else(|| anyhow!("must provide block producer config in mode: {:?}", mode))?;
            let mem_pool = mem_pool
                .clone()
                .ok_or_else(|| anyhow!("mem-pool must be enabled in mode: {:?}", mode))?;
            let wallet =
                wallet.ok_or_else(|| anyhow!("wallet must be enabled in mode: {:?}", mode))?;
            let poa = poa.ok_or_else(|| anyhow!("poa must be enabled in mode: {:?}", mode))?;
            let offchain_mock_context = {
                let ctx = offchain_mock_context.clone();
                let msg = "offchain mock require block producer config, wallet and poa in mode: ";
                ctx.ok_or_else(|| anyhow!("{} {:?}", msg, mode))?
            };
            let tests_control = if let NodeMode::Test = config.node_mode {
                Some(TestModeControl::new(
                    rpc_client.clone(),
                    Arc::clone(&poa),
                    store.clone(),
                ))
            } else {
                None
            };

            let cleaner = Arc::new(Cleaner::new(
                rpc_client.clone(),
                ckb_genesis_info.clone(),
                wallet,
            ));

            let wallet = Wallet::from_config(&block_producer_config.wallet_config)
                .with_context(|| "init wallet")?;

            // Challenger
            let challenger = Challenger::new(
                rollup_context,
                rpc_client.clone(),
                wallet,
                block_producer_config.clone(),
                config.debug.clone(),
                builtin_load_data,
                ckb_genesis_info.clone(),
                Arc::clone(&chain),
                Arc::clone(&poa),
                tests_control.clone(),
                Arc::clone(&cleaner),
                offchain_mock_context,
            );

            // Block Producer
            let block_producer = BlockProducer::create(
                rollup_config_hash,
                store.clone(),
                generator.clone(),
                Arc::clone(&chain),
                mem_pool,
                rpc_client.clone(),
                ckb_genesis_info,
                block_producer_config,
                config.debug.clone(),
                tests_control.clone(),
            )
            .with_context(|| "init block producer")?;

            (
                Some(block_producer),
                Some(challenger),
                tests_control,
                Some(cleaner),
            )
        }
    };

    // Transaction packager background service
    let mem_pool_batch = match mem_pool.as_ref() {
        Some(mem_pool) => {
            let inner = smol::block_on(mem_pool.lock()).inner();
            Some(MemPoolBatch::new(inner, Arc::clone(mem_pool)))
        }
        None => None,
    };

    // RPC registry
    let rpc_registry = Registry::new(
        store,
        mem_pool,
        generator,
        test_mode_control.map(Box::new),
        rollup_config,
        config.debug.clone(),
        Arc::clone(&chain),
        offchain_mock_context,
        config.mem_pool.clone(),
        config.node_mode,
        mem_pool_batch,
    );

    let (exit_sender, exit_recv) = async_channel::bounded(100);
    let handle = {
        let exit_sender = exit_sender.clone();
        move || {
            exit_sender.try_send(()).ok();
        }
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
            ckb_fixed_hash::H256::from_slice(rollup_config_hash.as_slice()).unwrap();
        log::info!("Rollup type script hash: {}", rollup_type_script_hash);
        log::info!("Rollup config hash: {}", rollup_config_hash);
    }

    log::info!("{:?} mode", config.node_mode);

    let chain_task = smol::spawn({
        let exit_sender = exit_sender.clone();
        async move {
            if let Err(err) = poll_loop(
                rpc_client,
                chain_updater,
                block_producer,
                challenger,
                cleaner,
                Duration::from_secs(3),
            )
            .await
            {
                log::error!("chain polling loop exit unexpected, error: {}", err);
            }
            if let Err(err) = exit_sender.send(()).await {
                log::error!("send exit signal error: {}", err)
            }
        }
    });
    let rpc_task = smol::spawn(async move {
        if let Err(err) = start_jsonrpc_server(rpc_address, rpc_registry).await {
            log::error!("Error running JSONRPC server: {:?}", err);
        }
        if let Err(err) = exit_sender.send(()).await {
            log::error!("send exit signal error: {}", err)
        }
    });

    smol::block_on(async {
        let _ = exit_recv.recv().await;
        log::info!("Exiting...");

        rpc_task.cancel().await;
        chain_task.cancel().await;
    });

    Ok(())
}

fn check_ckb_version(rpc_client: &RPCClient) -> Result<()> {
    let ckb_version = smol::block_on(rpc_client.get_ckb_version())?;
    let ckb_version = ckb_version.split('(').collect::<Vec<&str>>()[0].trim_end();
    if Version::parse(ckb_version)? < Version::parse(MIN_CKB_VERSION)? {
        return Err(anyhow!(
            "The version of CKB node {} is lower than the minimum version {}",
            ckb_version,
            MIN_CKB_VERSION
        ));
    }
    Ok(())
}

fn check_rollup_config_cell(
    block_producer_config: &BlockProducerConfig,
    rollup_config: &RollupConfig,
    rpc_client: &RPCClient,
) -> Result<()> {
    let rollup_config_cell = smol::block_on(
        rpc_client.get_cell(
            block_producer_config
                .rollup_config_cell_dep
                .out_point
                .clone()
                .into(),
        ),
    )?
    .ok_or_else(|| anyhow!("can't find rollup config cell"))?;
    let cell_data = RollupConfig::from_slice(&rollup_config_cell.data.to_vec())?;
    let eoa_set = rollup_config
        .allowed_eoa_type_hashes()
        .into_iter()
        .collect::<Vec<_>>();
    let contract_set = rollup_config
        .allowed_contract_type_hashes()
        .into_iter()
        .collect::<Vec<_>>();
    let unregistered_eoas = cell_data
        .allowed_eoa_type_hashes()
        .into_iter()
        .filter(|item| !eoa_set.contains(item))
        .collect::<Vec<_>>();
    let unregistered_contracts = cell_data
        .allowed_contract_type_hashes()
        .into_iter()
        .filter(|item| !contract_set.contains(item))
        .collect::<Vec<_>>();
    if !unregistered_eoas.is_empty() || !unregistered_contracts.is_empty() {
        return Err(anyhow!(
            "Unregistered eoa type hashes: {:#?}, \
            unregistered contract type hashes: {:#?}",
            unregistered_eoas,
            unregistered_contracts
        ));
    }
    Ok(())
}

fn check_locks(
    block_producer_config: &BlockProducerConfig,
    rollup_config: &RollupConfig,
) -> Result<()> {
    let zeros = ckb_fixed_hash::H256([0u8; 32]);

    // check burn lock
    if zeros != block_producer_config.challenger_config.burn_lock.code_hash {
        return Err(anyhow!(
            "[block_producer.challenger.burn_lock.code_hash] is expected to be zero"
        ));
    }

    let burn_lock_hash = {
        let script: gw_types::packed::Script = block_producer_config
            .challenger_config
            .burn_lock
            .clone()
            .into();
        script.hash().pack()
    };
    if burn_lock_hash != rollup_config.burn_lock_hash() {
        return Err(anyhow!("[block_producer.challenge.burn_lock] ({}) isn't match rollup config's burn_lock_hash ({})", burn_lock_hash, rollup_config.burn_lock_hash()));
    }

    // check challenge lock
    if zeros
        == block_producer_config
            .challenger_config
            .rewards_receiver_lock
            .code_hash
    {
        return Err(anyhow!(
            "[block_producer.challenger.rewards_receiver_lock.code_hash] shouldn't be zero"
        ));
    }

    // check wallet lock
    if zeros == block_producer_config.wallet_config.lock.code_hash {
        return Err(anyhow!(
            "[block_producer.wallet.lock.code_hash] shouldn't be zero"
        ));
    }
    if block_producer_config.wallet_config.lock
        == block_producer_config
            .challenger_config
            .rewards_receiver_lock
    {
        return Err(anyhow!(
            "[block_producer.challenger.rewards_receiver_lock] and [block_producer.wallet.lock] have the same address, which is not recommended"
        ));
    }
    Ok(())
}

fn is_hardfork_switch_eq(l: &HardForkSwitch, r: &HardForkSwitch) -> bool {
    l.rfc_0028() == r.rfc_0028()
        && l.rfc_0029() == r.rfc_0029()
        && l.rfc_0030() == r.rfc_0030()
        && l.rfc_0031() == r.rfc_0031()
        && l.rfc_0032() == r.rfc_0032()
        && l.rfc_0036() == r.rfc_0036()
        && l.rfc_0038() == r.rfc_0038()
}
