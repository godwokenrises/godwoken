use crate::{
    block_producer::{BlockProducer, BlockProducerCreateArgs},
    challenger::{Challenger, ChallengerNewArgs},
    cleaner::Cleaner,
    poller::ChainUpdater,
    test_mode_control::TestModeControl,
    types::ChainEvent,
    withdrawal_unlocker::FinalizedWithdrawalUnlocker,
};
use anyhow::{anyhow, bail, Context, Result};
use ckb_types::core::hardfork::HardForkSwitch;
use gw_chain::chain::Chain;
use gw_challenge::offchain::{OffChainMockContext, OffChainMockContextBuildArgs};
use gw_ckb_hardfork::{GLOBAL_CURRENT_EPOCH_NUMBER, GLOBAL_HARDFORK_SWITCH, GLOBAL_VM_VERSION};
use gw_common::{blake2b::new_blake2b, H256};
use gw_config::{BlockProducerConfig, Config, NodeMode};
use gw_db::migrate::open_or_create_db;
use gw_dynamic_config::manager::DynamicConfigManager;
use gw_generator::{
    account_lock_manage::{
        secp256k1::{Secp256k1Eth, Secp256k1Tron},
        AccountLockManage,
    },
    backend_manage::BackendManage,
    genesis::init_genesis,
    ArcSwap, Generator,
};
use gw_mem_pool::{
    default_provider::DefaultMemPoolProvider,
    pool::{MemPool, MemPoolCreateArgs},
    spawn_sub_mem_pool_task,
    traits::MemPoolErrorTxHandler,
};
use gw_poa::PoA;
use gw_rpc_client::{
    ckb_client::CKBClient, contract::ContractsCellDepManager, indexer_client::CKBIndexerClient,
    rpc_client::RPCClient,
};
use gw_rpc_server::{
    registry::{Registry, RegistryArgs},
    server::start_jsonrpc_server,
};
use gw_rpc_ws_server::{notify_controller::NotifyService, server::start_jsonrpc_ws_server};
use gw_store::Store;
use gw_types::{
    bytes::Bytes,
    offchain::RollupContext,
    packed::{Byte32, CellDep, NumberHash, RollupConfig, Script},
    prelude::*,
};
use gw_utils::{genesis_info::CKBGenesisInfo, since::EpochNumberWithFraction, wallet::Wallet};
use gw_web3_indexer::{ErrorReceiptIndexer, Web3Indexer};
use semver::Version;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    ConnectOptions,
};
use std::{
    collections::HashMap,
    net::{SocketAddr, ToSocketAddrs},
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};
use tokio::{
    spawn,
    sync::{broadcast, mpsc, Mutex},
};

const MIN_CKB_VERSION: &str = "0.40.0";
const SMOL_THREADS_ENV_VAR: &str = "SMOL_THREADS";
const DEFAULT_RUNTIME_THREADS: usize = 8;
const EVENT_TIMEOUT_SECONDS: u64 = 30;

struct ChainTaskContext {
    chain_updater: ChainUpdater,
    block_producer: Option<BlockProducer>,
    challenger: Option<Challenger>,
    withdrawal_unlocker: Option<FinalizedWithdrawalUnlocker>,
    cleaner: Option<Arc<Cleaner>>,
}
struct ChainTask {
    rpc_client: RPCClient,
    poll_interval: Duration,
    ctx: Arc<tokio::sync::Mutex<ChainTaskContext>>,
    shutdown_event: broadcast::Receiver<()>,
    _shutdown_send: mpsc::Sender<()>,
}

impl ChainTask {
    fn create(
        rpc_client: RPCClient,
        poll_interval: Duration,
        ctx: ChainTaskContext,
        shutdown_send: mpsc::Sender<()>,
        shutdown_event: broadcast::Receiver<()>,
    ) -> Self {
        let ctx = Arc::new(tokio::sync::Mutex::new(ctx));
        Self {
            rpc_client,
            poll_interval,
            ctx,
            _shutdown_send: shutdown_send,
            shutdown_event,
        }
    }

    async fn run(mut self) -> Result<()> {
        // get tip
        let (mut tip_number, mut tip_hash) = {
            let tip = self.rpc_client.get_tip().await?;
            let tip_number: u64 = tip.number().unpack();
            let tip_hash: H256 = tip.block_hash().unpack();
            (tip_number, tip_hash)
        };

        let mut last_event_time = Instant::now();

        loop {
            // Exit if shutdown event is received.
            if self.shutdown_event.try_recv().is_ok() {
                log::info!("ChainTask existed successfully");
                return Ok(());
            }
            if let Some(block) = self.rpc_client.get_block_by_number(tip_number + 1).await? {
                let raw_header = block.header().raw();
                let new_block_number = raw_header.number().unpack();
                let new_block_hash = block.header().hash();
                assert_eq!(
                    new_block_number,
                    tip_number + 1,
                    "should be the same number"
                );
                let event = if raw_header.parent_hash().as_slice() == tip_hash.as_slice() {
                    // received new layer1 block
                    log::info!(
                        "received new layer1 block {}, {}",
                        new_block_number,
                        hex::encode(new_block_hash),
                    );
                    ChainEvent::NewBlock {
                        block: block.clone(),
                    }
                } else {
                    // layer1 reverted
                    log::info!(
                        "layer1 reverted current tip: {}, {} -> new block: {}, {}",
                        tip_number,
                        hex::encode(tip_hash.as_slice()),
                        new_block_number,
                        hex::encode(new_block_hash),
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
                let ctx = self.ctx.clone();
                let mut ctx = ctx.lock().await;

                if let Some(ref mut withdrawal_unlocker) = ctx.withdrawal_unlocker {
                    if let Err(err) = withdrawal_unlocker.handle_event(&event).await {
                        log::error!("[unlock withdrawal] {}", err);
                    }
                }

                if let Err(err) = ctx.chain_updater.handle_event(event.clone()).await {
                    if is_l1_query_error(&err) {
                        log::error!("[polling] chain_updater event: {} error: {}", event, err);
                        tokio::time::sleep(self.poll_interval).await;
                        continue;
                    }
                    bail!(
                        "Error occurred when polling chain_updater, event: {}, error: {}",
                        event,
                        err
                    );
                }

                if let Some(ref mut challenger) = ctx.challenger {
                    if let Err(err) = challenger.handle_event(event.clone()).await {
                        if is_l1_query_error(&err) {
                            log::error!("[polling] challenger event: {} error: {}", event, err);
                            tokio::time::sleep(self.poll_interval).await;
                            continue;
                        }
                        bail!(
                            "Error occurred when polling challenger, event: {}, error: {}",
                            event,
                            err
                        );
                    }
                }

                if let Some(ref mut block_producer) = ctx.block_producer {
                    if let Err(err) = block_producer.handle_event(event.clone()).await {
                        if is_l1_query_error(&err) {
                            log::error!("[polling] block producer event: {} error: {}", event, err);
                            tokio::time::sleep(self.poll_interval).await;
                            continue;
                        }
                        bail!(
                            "Error occurred when polling block_producer, event: {}, error: {}",
                            event,
                            err
                        );
                    }
                }

                if let Some(ref cleaner) = ctx.cleaner {
                    if let Err(err) = cleaner.handle_event(event.clone()).await {
                        if is_l1_query_error(&err) {
                            log::error!("[polling] cleaner event: {} error: {}", event, err);
                            tokio::time::sleep(self.poll_interval).await;
                            continue;
                        }
                        bail!(
                            "Error occurred when polling cleaner, event: {}, error: {}",
                            event,
                            err
                        );
                    }
                }

                // update tip
                tip_number = new_block_number;
                tip_hash = block.header().hash().into();

                // update global hardfork info
                let hardfork_switch = self.rpc_client.get_hardfork_switch().await?;
                let rfc0032_epoch_number = hardfork_switch.rfc_0032();
                let global_hardfork_switch = GLOBAL_HARDFORK_SWITCH.load();
                if !is_hardfork_switch_eq(&*global_hardfork_switch, &hardfork_switch) {
                    GLOBAL_HARDFORK_SWITCH.store(Arc::new(hardfork_switch));
                }

                // when switching the epoch, update the tip epoch number and VM version
                let tip_epoch = {
                    let tip_epoch: u64 = block.header().raw().epoch().unpack();
                    EpochNumberWithFraction::from_full_value(tip_epoch)
                };
                if tip_epoch.index() == 0 || tip_epoch.index() == tip_epoch.length() - 1 {
                    let vm_version: u32 = if tip_epoch.number() >= rfc0032_epoch_number {
                        1
                    } else {
                        0
                    };
                    GLOBAL_CURRENT_EPOCH_NUMBER.store(tip_epoch.number(), Ordering::SeqCst);
                    GLOBAL_VM_VERSION.store(vm_version, Ordering::SeqCst);
                }
                last_event_time = Instant::now();
            } else {
                log::debug!(
                    "Not found layer1 block #{} sleep {}s then retry",
                    tip_number + 1,
                    self.poll_interval.as_secs()
                );
                let seconds_since_last_event = last_event_time.elapsed().as_secs();
                if seconds_since_last_event > EVENT_TIMEOUT_SECONDS {
                    log::warn!(
                    "Can't find layer1 block update in {}s. last block is #{}({}) CKB node may out of sync",
                    seconds_since_last_event,
                    tip_number,
                    {
                        let hash: Byte32 =  tip_hash.pack();
                        hash
                    }
                );
                }
                tokio::time::sleep(self.poll_interval).await;
            }
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
    pub contracts_dep_manager: Option<ContractsCellDepManager>,
    pub dynamic_config_manager: Arc<ArcSwap<DynamicConfigManager>>,
}

impl BaseInitComponents {
    #[allow(deprecated)]
    pub async fn init(config: &Config, skip_config_check: bool) -> Result<Self> {
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
            let indexer_client = CKBIndexerClient::with_url(&config.rpc_client.indexer_url)?;
            let ckb_client = CKBClient::with_url(&config.rpc_client.ckb_url)?;
            let rollup_type_script =
                ckb_types::packed::Script::new_unchecked(rollup_type_script.as_bytes());
            RPCClient::new(
                rollup_type_script,
                rollup_context.clone(),
                ckb_client,
                indexer_client,
            )
        };

        let opt_block_producer_config = config.block_producer.as_ref();
        let mut contracts_dep_manager = None;
        if let Some(block_producer_config) = opt_block_producer_config {
            use gw_rpc_client::contract::{check_script, query_type_script_from_old_config};
            let mut script_config = config.consensus.contract_type_scripts.clone();
            let rollup_type_script = &config.chain.rollup_type_script;

            if check_script(&script_config, &rollup_config, rollup_type_script).is_err() {
                let now = Instant::now();
                script_config =
                    query_type_script_from_old_config(&rpc_client, block_producer_config).await?;
                log::trace!("[contracts dep] old config {}ms", now.elapsed().as_millis());

                check_script(&script_config, &rollup_config, rollup_type_script)?;
            }

            contracts_dep_manager =
                Some(ContractsCellDepManager::build(rpc_client.clone(), script_config).await?);
        }

        if !skip_config_check {
            check_ckb_version(&rpc_client).await?;
            // TODO: check ckb indexer version
            if NodeMode::ReadOnly != config.node_mode {
                let block_producer_config =
                    opt_block_producer_config.ok_or_else(|| anyhow!("not set block producer"))?;
                check_rollup_config_cell(block_producer_config, &rollup_config, &rpc_client)
                    .await?;
                check_locks(block_producer_config, &rollup_config)?;
            }
        }

        // Open store
        let timer = Instant::now();
        let store = if config.store.path.as_os_str().is_empty() {
            log::warn!("config.store.path is blank, using temporary store");
            Store::open_tmp().with_context(|| "init store")?
        } else {
            Store::new(open_or_create_db(&config.store)?)
        };
        let elapsed_ms = timer.elapsed().as_millis();
        log::debug!("Open rocksdb costs: {}ms.", elapsed_ms);

        let secp_data: Bytes = {
            let out_point = config.genesis.secp_data_dep.out_point.clone();
            rpc_client
                .get_transaction(out_point.tx_hash.0.into())
                .await?
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

        let dynamic_config_manager = Arc::new(ArcSwap::from_pointee(DynamicConfigManager::create(
            config.clone(),
        )));

        //Reload config
        if let Some(res) = gw_dynamic_config::try_reload(dynamic_config_manager.clone()).await {
            log::info!("Reload dynamic config: {:?}", res);
        }
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
            ))
        };

        let ckb_genesis_info = {
            let ckb_genesis = rpc_client
                .get_block_by_number(0)
                .await?
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
            contracts_dep_manager,
            dynamic_config_manager,
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
        Arc::new(tokio::sync::Mutex::new(poa))
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
        let contracts_dep_manager = self
            .contracts_dep_manager
            .clone()
            .ok_or_else(|| anyhow!("expect contracts dep manager"))?;

        let build_args = OffChainMockContextBuildArgs {
            rpc_client: &self.rpc_client,
            poa,
            rollup_context: self.rollup_context.clone(),
            wallet,
            config: block_producer_config.clone(),
            ckb_genesis_info,
            builtin_load_data: self.builtin_load_data.clone(),
            contracts_dep_manager,
        };

        OffChainMockContext::build(build_args).await
    }
}

pub async fn run(config: Config, skip_config_check: bool) -> Result<()> {
    // Set up sentry.
    let _guard = match &config.sentry_dsn.as_ref() {
        Some(sentry_dsn) => sentry::init((
            sentry_dsn.as_str(),
            sentry::ClientOptions {
                release: sentry::release_name!(),
                ..Default::default()
            },
        )),
        None => sentry::init(()),
    };

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

    let base = BaseInitComponents::init(&config, skip_config_check).await?;
    let (mem_pool, wallet, poa, offchain_mock_context, pg_pool, err_receipt_notify_ctrl) =
        match config.block_producer.as_ref() {
            Some(block_producer_config) => {
                let wallet = Wallet::from_config(&block_producer_config.wallet_config)
                    .with_context(|| "init wallet")?;
                let poa = base.init_poa(&wallet, block_producer_config);
                let offchain_mock_context = {
                    let poa = poa.lock().await;
                    base.init_offchain_mock_context(&poa, block_producer_config)
                        .await?
                };
                let mem_pool_provider = DefaultMemPoolProvider::new(
                    base.rpc_client.clone(),
                    Arc::clone(&poa),
                    base.store.clone(),
                );
                let pg_pool = match config.web3_indexer.as_ref() {
                    Some(web3_indexer_config) => {
                        let mut opts: PgConnectOptions =
                            web3_indexer_config.database_url.parse()?;
                        opts.log_statements(log::LevelFilter::Debug)
                            .log_slow_statements(log::LevelFilter::Warn, Duration::from_secs(5));
                        let pool = PgPoolOptions::new()
                            .max_connections(5)
                            .connect_with(opts)
                            .await?;
                        Some(pool)
                    }
                    None => None,
                };
                let error_tx_handler = pg_pool.clone().map(|pool| {
                    Box::new(ErrorReceiptIndexer::new(pool))
                        as Box<dyn MemPoolErrorTxHandler + Send + Sync>
                });
                let notify_controller = {
                    let opt_ws_listen = config.rpc_server.err_receipt_ws_listen.as_ref();
                    opt_ws_listen.map(|_| NotifyService::new().start())
                };
                let mem_pool = {
                    let args = MemPoolCreateArgs {
                        block_producer_id: block_producer_config.account_id,
                        store: base.store.clone(),
                        generator: base.generator.clone(),
                        provider: Box::new(mem_pool_provider),
                        error_tx_handler,
                        error_tx_receipt_notifier: notify_controller.clone(),
                        config: config.mem_pool.clone(),
                        node_mode: config.node_mode,
                        dynamic_config_manager: base.dynamic_config_manager.clone(),
                    };
                    Arc::new(Mutex::new(
                        MemPool::create(args)
                            .await
                            .with_context(|| "create mem-pool")?,
                    ))
                };
                (
                    Some(mem_pool),
                    Some(wallet),
                    Some(poa),
                    Some(offchain_mock_context),
                    pg_pool,
                    notify_controller,
                )
            }
            None => (None, None, None, None, None, None),
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
        contracts_dep_manager,
        dynamic_config_manager,
        ..
    } = base;

    // check state db
    {
        let t = Instant::now();
        store.check_state()?;
        log::info!("Check state db done: {}ms", t.elapsed().as_millis());
    }
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
            log::info!("Check web3 indexing...");
            web3_indexer.store_genesis(&store).await?;
            web3_indexer.fix_missing_blocks(&store).await?;
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

    let (block_producer, challenger, test_mode_control, withdrawal_unlocker, cleaner) = match config
        .node_mode
    {
        NodeMode::ReadOnly => {
            if let Some(sync_mem_block_config) = &config.mem_pool.subscribe {
                match &mem_pool {
                    Some(mem_pool) => {
                        spawn_sub_mem_pool_task(mem_pool.clone(), sync_mem_block_config.clone())?;
                    }
                    None => {
                        log::warn!("Failed to init sync mem block, because mem_pool is None.");
                    }
                }
            }
            (None, None, None, None, None)
        }
        mode => {
            let block_producer_config = config
                .block_producer
                .clone()
                .ok_or_else(|| anyhow!("must provide block producer config in mode: {:?}", mode))?;
            let contracts_dep_manager =
                contracts_dep_manager.ok_or_else(|| anyhow!("must build contracts dep"))?;
            let mem_pool = mem_pool
                .clone()
                .ok_or_else(|| anyhow!("mem-pool must be enabled in mode: {:?}", mode))?;
            let wallet =
                wallet.ok_or_else(|| anyhow!("wallet must be enabled in mode: {:?}", mode))?;
            let offchain_mock_context = {
                let ctx = offchain_mock_context;
                let msg = "offchain mock require block producer config, wallet and poa in mode: ";
                ctx.ok_or_else(|| anyhow!("{} {:?}", msg, mode))?
            };
            let poa = poa.ok_or_else(|| anyhow!("poa must be enabled in mode: {:?}", mode))?;
            let tests_control = if let NodeMode::Test = config.node_mode {
                Some(TestModeControl::new(
                    rpc_client.clone(),
                    Arc::clone(&poa),
                    store.clone(),
                ))
            } else {
                None
            };

            let unlocker_wallet = match block_producer_config
                .withdrawal_unlocker_wallet_config
                .as_ref()
            {
                Some(wallet_config) => {
                    Wallet::from_config(wallet_config).with_context(|| "init unlocker wallet")?
                }
                None => {
                    log::info!("[unlock withdrawal] reuse block producer wallet");
                    Wallet::from_config(&block_producer_config.wallet_config)
                        .with_context(|| "init unlocker wallet")?
                }
            };

            let withdrawal_unlocker = FinalizedWithdrawalUnlocker::new(
                rpc_client.clone(),
                ckb_genesis_info.clone(),
                contracts_dep_manager.clone(),
                unlocker_wallet,
                config.debug.clone(),
            );

            let cleaner = Arc::new(Cleaner::new(
                rpc_client.clone(),
                ckb_genesis_info.clone(),
                wallet,
            ));

            let wallet = Wallet::from_config(&block_producer_config.wallet_config)
                .with_context(|| "init wallet")?;

            // Challenger
            let args = ChallengerNewArgs {
                rollup_context,
                rpc_client: rpc_client.clone(),
                wallet,
                config: block_producer_config.clone(),
                debug_config: config.debug.clone(),
                builtin_load_data,
                ckb_genesis_info: ckb_genesis_info.clone(),
                chain: Arc::clone(&chain),
                poa: Arc::clone(&poa),
                tests_control: tests_control.clone(),
                cleaner: Arc::clone(&cleaner),
                offchain_mock_context,
                contracts_dep_manager: contracts_dep_manager.clone(),
            };
            let challenger = Challenger::new(args);

            // Block Producer
            let create_args = BlockProducerCreateArgs {
                rollup_config_hash,
                store: store.clone(),
                generator: generator.clone(),
                chain: Arc::clone(&chain),
                mem_pool,
                rpc_client: rpc_client.clone(),
                ckb_genesis_info,
                config: block_producer_config,
                debug_config: config.debug.clone(),
                tests_control: tests_control.clone(),
                contracts_dep_manager,
            };
            let block_producer =
                BlockProducer::create(create_args).with_context(|| "init block producer")?;

            (
                Some(block_producer),
                Some(challenger),
                tests_control,
                Some(withdrawal_unlocker),
                Some(cleaner),
            )
        }
    };

    // RPC registry
    let args = RegistryArgs {
        store,
        mem_pool,
        generator,
        tests_rpc_impl: test_mode_control.map(Box::new),
        rollup_config,
        mem_pool_config: config.mem_pool.clone(),
        node_mode: config.node_mode,
        rpc_client: rpc_client.clone(),
        send_tx_rate_limit: config.dynamic_config.rpc_config.send_tx_rate_limit.clone(),
        server_config: config.rpc_server.clone(),
        dynamic_config_manager,
        last_submitted_tx_hash: block_producer
            .as_ref()
            .map(|bp| bp.last_submitted_tx_hash()),
        withdrawal_to_v1_config: config.withdrawal_to_v1_config.clone(),
    };

    let rpc_registry = Registry::create(args).await;

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
    //Graceful shutdown event. If all the shutdown_sends get dropped, then we can shutdown gracefully.
    let (shutdown_send, mut shutdown_recv) = mpsc::channel(1);
    //Broadcase shutdown event.
    let (shutdown_event, shutdown_event_recv) = broadcast::channel(1);

    let chain_task = spawn({
        let ctx = ChainTaskContext {
            chain_updater,
            block_producer,
            challenger,
            withdrawal_unlocker,
            cleaner,
        };
        let chain_task = ChainTask::create(
            rpc_client,
            Duration::from_secs(3),
            ctx,
            shutdown_send.clone(),
            shutdown_event_recv,
        );
        async move {
            if let Err(err) = chain_task.run().await {
                log::error!("chain polling loop exit unexpected, error: {}", err);
            }
        }
    });

    let sub_shutdown = shutdown_event.subscribe();
    let rpc_shutdown_send = shutdown_send.clone();
    let rpc_task = spawn(async move {
        if let Err(err) =
            start_jsonrpc_server(rpc_address, rpc_registry, rpc_shutdown_send, sub_shutdown).await
        {
            log::error!("Error running JSONRPC server: {:?}", err);
        }
    });

    let mut rpc_ws_task = None;
    if let Some(notify_controller) = err_receipt_notify_ctrl {
        let rpc_ws_addr = {
            let ws_listen = config.rpc_server.err_receipt_ws_listen.as_ref();
            ws_listen.expect("err receipt ws listen").to_owned()
        };
        let ws_shutdown_send = shutdown_send.clone();
        let sub_shutdown = shutdown_event.subscribe();
        rpc_ws_task = Some(spawn(async move {
            if let Err(err) = start_jsonrpc_ws_server(
                &rpc_ws_addr,
                notify_controller.clone(),
                ws_shutdown_send,
                sub_shutdown,
            )
            .await
            {
                log::error!("Error running JSONRPC WebSockert server: {:?}", err);
            }
            notify_controller.stop();
        }));
    }

    match rpc_ws_task {
        Some(rpc_ws_task) => {
            tokio::select! {
                _ = sigint_or_sigterm() => { },
                _ = chain_task => {},
                _ = rpc_ws_task => {},
                _ = rpc_task => {},
            }
        }
        None => {
            tokio::select! {
                _ = sigint_or_sigterm() => { },
                _ = chain_task => {},
                _ = rpc_task => {},
            };
        }
    }

    //If any task is out of running, broadcast shutdown event.
    log::info!("send shutdown event");
    if let Err(err) = shutdown_event.send(()) {
        log::error!("Failed to brodcast error message: {:?}", err);
    }

    // Make sure all the senders are dropped.
    drop(shutdown_send);

    // When every sender has gone out of scope, the recv call
    // will return with an error. We ignore the error. Just
    // make sure we can hit this line.
    let _ = shutdown_recv.recv().await;
    log::info!("Exiting...");

    Ok(())
}

async fn check_ckb_version(rpc_client: &RPCClient) -> Result<()> {
    let ckb_version = rpc_client.get_ckb_version().await?;
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

async fn check_rollup_config_cell(
    block_producer_config: &BlockProducerConfig,
    rollup_config: &RollupConfig,
    rpc_client: &RPCClient,
) -> Result<()> {
    let rollup_config_cell = rpc_client
        .get_cell(
            block_producer_config
                .rollup_config_cell_dep
                .out_point
                .clone()
                .into(),
        )
        .await?
        .and_then(|cell_with_status| cell_with_status.cell)
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

fn is_l1_query_error(err: &anyhow::Error) -> bool {
    use crate::poller::QueryL1TxError;
    use gw_rpc_client::error::RPCRequestError;

    // TODO: filter rpc request method?
    err.downcast_ref::<RPCRequestError>().is_some()
        || err.downcast_ref::<QueryL1TxError>().is_some()
}

async fn sigint_or_sigterm() {
    let int = tokio::signal::ctrl_c();
    #[cfg(unix)]
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("creating SIGTERM stream");
    #[cfg(unix)]
    tokio::select! {
        _ = int => {}
        _ = term.recv() => {}
    }
    #[cfg(not(unix))]
    let _ = int.await;

    log::info!("received sigint or sigterm, shutting down");
}
