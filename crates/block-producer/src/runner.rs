use crate::{
    block_producer::{BlockProducer, BlockProducerCreateArgs},
    block_sync_client::{block_sync_client_protocol, BlockSyncClient, P2PStream},
    chain_updater::ChainUpdater,
    challenger::{Challenger, ChallengerNewArgs},
    cleaner::Cleaner,
    psc::{PSCContext, ProduceSubmitConfirm},
    test_mode_control::TestModeControl,
    types::ChainEvent,
    withdrawal_unlocker::FinalizedWithdrawalUnlocker,
};
use anyhow::{anyhow, bail, Context, Result};
use futures::future::OptionFuture;
use gw_chain::chain::Chain;
use gw_challenge::offchain::{OffChainMockContext, OffChainMockContextBuildArgs};
use gw_common::{blake2b::new_blake2b, registry_address::RegistryAddress, H256};
use gw_config::{BlockProducerConfig, Config, NodeMode};
use gw_db::migrate::open_or_create_db;
use gw_dynamic_config::manager::DynamicConfigManager;
use gw_generator::{
    account_lock_manage::{secp256k1::Secp256k1Eth, AccountLockManage},
    backend_manage::BackendManage,
    genesis::init_genesis,
    ArcSwap, Generator,
};
use gw_mem_pool::{
    account_creator::AccountCreator,
    block_sync_server::{block_sync_server_protocol, BlockSyncServerState},
    default_provider::DefaultMemPoolProvider,
    pool::{MemPool, MemPoolCreateArgs},
};
use gw_p2p_network::P2PNetwork;
use gw_polyjuice_sender_recover::recover::PolyjuiceSenderRecover;
use gw_rpc_client::{
    ckb_client::CKBClient, contract::ContractsCellDepManager, error::RPCRequestError,
    indexer_client::CKBIndexerClient, rpc_client::RPCClient,
};
use gw_rpc_server::{
    registry::{Registry, RegistryArgs},
    server::start_jsonrpc_server,
};
use gw_store::Store;
use gw_types::{
    bytes::Bytes,
    core::AllowedEoaType,
    packed::{Byte32, CellDep, NumberHash, RollupConfig, Script},
    prelude::*,
};
use gw_utils::RollupContext;
use gw_utils::{
    exponential_backoff::ExponentialBackoff, genesis_info::CKBGenesisInfo, liveness::Liveness,
    local_cells::LocalCellsManager, wallet::Wallet,
};
use semver::Version;
use std::{
    collections::HashMap,
    net::{SocketAddr, ToSocketAddrs},
    sync::Arc,
    time::{Duration, Instant},
};
use tentacle::service::ProtocolMeta;
use tokio::{
    spawn,
    sync::{broadcast, mpsc, Mutex},
};
use tracing::{info_span, instrument};

const MIN_CKB_VERSION: &str = "0.40.0";
const EVENT_TIMEOUT_SECONDS: u64 = 30;

struct ChainTaskContext {
    challenger: Option<Challenger>,
    withdrawal_unlocker: Option<FinalizedWithdrawalUnlocker>,
    cleaner: Option<Arc<Cleaner>>,
}

struct ChainTaskRunStatus {
    opt_tip_number_hash: Option<(u64, H256)>,
    last_event_time: Instant,
}

impl Default for ChainTaskRunStatus {
    fn default() -> Self {
        ChainTaskRunStatus {
            opt_tip_number_hash: None,
            last_event_time: Instant::now(),
        }
    }
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

    #[instrument(skip_all, fields(tip_number = tip_number, tip_hash = %tip_hash.pack()))]
    async fn sync_next(
        &self,
        tip_number: u64,
        tip_hash: H256,
        last_event_time: &Instant,
    ) -> Result<Option<(u64, H256)>> {
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
                    hex::encode(new_block_hash)
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

            if let Some(ref mut challenger) = ctx.challenger {
                if let Err(err) = challenger.handle_event(event.clone()).await {
                    if is_l1_query_error(&err) {
                        log::error!("[polling] challenger event: {} error: {}", event, err);
                        return Ok(None);
                    }
                    if err.to_string().contains("TransactionFailedToResolve") {
                        log::info!("[polling] challenger outdated rollup status, wait update");
                    } else {
                        bail!(
                            "Error occurred when polling challenger, event: {}, error: {}",
                            event,
                            err
                        );
                    }
                }
            }

            if let Some(ref cleaner) = ctx.cleaner {
                if let Err(err) = cleaner.handle_event(event.clone()).await {
                    if is_l1_query_error(&err) {
                        log::error!("[polling] cleaner event: {} error: {}", event, err);
                        return Ok(None);
                    }
                    bail!(
                        "Error occurred when polling cleaner, event: {}, error: {}",
                        event,
                        err
                    );
                }
            }

            // update tip
            Ok(Some((new_block_number, block.header().hash().into())))
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
            Ok(None)
        }
    }

    // How to get tip_number and tip_hash only once? then loop chain task run only?
    #[instrument(skip_all, err(Debug))]
    async fn run(&mut self, status: &ChainTaskRunStatus) -> Result<ChainTaskRunStatus> {
        // get tip
        let (tip_number, tip_hash) = match status.opt_tip_number_hash {
            Some((number, hash)) => (number, hash),
            None => {
                let tip = self.rpc_client.get_tip().await?;
                let tip_number: u64 = tip.number().unpack();
                let tip_hash: H256 = tip.block_hash().unpack();
                (tip_number, tip_hash)
            }
        };

        let opt_tip_number_hash = self
            .sync_next(tip_number, tip_hash, &status.last_event_time)
            .await?;

        let updated_status = ChainTaskRunStatus {
            opt_tip_number_hash: opt_tip_number_hash
                .or_else(|| status.opt_tip_number_hash.to_owned()),
            last_event_time: Instant::now(),
        };

        Ok(updated_status)
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
    pub async fn init(config: &Config, skip_config_check: bool) -> Result<Self> {
        let rollup_config: RollupConfig = config.genesis.rollup_config.clone().into();
        let rollup_context = RollupContext {
            rollup_config: rollup_config.clone(),
            rollup_script_hash: {
                let rollup_script_hash: [u8; 32] = config.genesis.rollup_type_hash.clone().into();
                rollup_script_hash.into()
            },
            fork_config: config.fork.clone(),
        };
        let rollup_type_script: Script = config.chain.rollup_type_script.clone().into();
        let rpc_client = {
            let indexer_client = CKBIndexerClient::with_url(&config.rpc_client.indexer_url)?;
            let ckb_client = CKBClient::with_url(&config.rpc_client.ckb_url)?;
            let rollup_type_script =
                ckb_types::packed::Script::new_unchecked(rollup_type_script.as_bytes());
            RPCClient::new(
                rollup_type_script,
                rollup_context.rollup_config.clone(),
                ckb_client,
                indexer_client,
            )
        };

        let opt_block_producer_config = config.block_producer.as_ref();
        let mut contracts_dep_manager = None;
        if opt_block_producer_config.is_some() {
            use gw_rpc_client::contract::check_script;
            let script_config = config.consensus.contract_type_scripts.clone();
            let rollup_type_script = &config.chain.rollup_type_script;

            check_script(&script_config, &rollup_config, rollup_type_script)?;
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
                .ckb
                .get_transaction(out_point.tx_hash.0.into())
                .await?
                .ok_or_else(|| anyhow!("can not found transaction: {:?}", out_point.tx_hash))?
                .raw()
                .outputs_data()
                .get(out_point.index.value() as usize)
                .expect("get secp output data")
                .raw_data()
        };

        let genesis_tx_hash = config
            .chain
            .genesis_committed_info
            .transaction_hash
            .clone()
            .into();
        init_genesis(&store, &config.genesis, &genesis_tx_hash, secp_data.clone())
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
            let backend_manage = BackendManage::from_config(config.fork.backend_forks.clone())
                .with_context(|| "config backends")?;
            let mut account_lock_manage = AccountLockManage::default();
            let allowed_eoa_type_hashes = rollup_config.as_reader().allowed_eoa_type_hashes();
            let eth_lock_script_type_hash = allowed_eoa_type_hashes
                .iter()
                .find(|th| th.type_().to_entity() == AllowedEoaType::Eth.into())
                .ok_or_else(|| anyhow!("Eth: No allowed EoA type hashes in the rollup config"))?;
            account_lock_manage.register_lock_algorithm(
                eth_lock_script_type_hash.hash().unpack(),
                Arc::new(Secp256k1Eth::default()),
            );
            Arc::new(Generator::new(
                backend_manage,
                account_lock_manage,
                rollup_context.clone(),
                config.contract_log_config.clone(),
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

    pub async fn init_offchain_mock_context(
        &self,
        block_producer_config: &BlockProducerConfig,
    ) -> Result<Option<OffChainMockContext>> {
        let wallet = match block_producer_config.wallet_config {
            Some(ref c) => Wallet::from_config(c).with_context(|| "offchain init wallet")?,
            None => return Ok(None),
        };
        let ckb_genesis_info = gw_challenge::offchain::CKBGenesisInfo {
            sighash_dep: self.ckb_genesis_info.sighash_dep(),
        };
        let contracts_dep_manager = self
            .contracts_dep_manager
            .clone()
            .ok_or_else(|| anyhow!("expect contracts dep manager"))?;

        let build_args = OffChainMockContextBuildArgs {
            rpc_client: &self.rpc_client,
            rollup_context: self.rollup_context.clone(),
            wallet,
            config: block_producer_config.clone(),
            ckb_genesis_info,
            builtin_load_data: self.builtin_load_data.clone(),
            contracts_dep_manager,
        };

        let ctx = OffChainMockContext::build(build_args).await?;
        Ok(Some(ctx))
    }
}

pub async fn run(config: Config, skip_config_check: bool) -> Result<()> {
    spawn_starvation_detector();

    // Set up runtim monitor.
    #[cfg(tokio_unstable)]
    {
        let runtime_monitor =
            tokio_metrics::RuntimeMonitor::new(&tokio::runtime::Handle::current());
        {
            tokio::spawn(async move {
                log::info!("Tokio runtime monitor is set up!");
                for interval in runtime_monitor.intervals() {
                    log::info!("runtime monitor: {:#?}", interval);
                    tokio::time::sleep(Duration::from_secs(10)).await;
                }
            });
        }
    }
    let base = BaseInitComponents::init(&config, skip_config_check).await?;

    let has_block_producer_and_p2p =
        config.block_producer.is_some() && config.p2p_network_config.is_some();
    let block_sync_server_state = if has_block_producer_and_p2p {
        Some(Arc::new(std::sync::Mutex::new(BlockSyncServerState::new(
            &config.sync_server,
        ))))
    } else {
        None
    };

    let (mem_pool, wallet, offchain_mock_context) = match config.block_producer.as_ref() {
        Some(block_producer_config) => {
            let opt_wallet = block_producer_config
                .wallet_config
                .as_ref()
                .map(|c| Wallet::from_config(c).with_context(|| "init block producer wallet"))
                .transpose()?;
            let opt_offchain_mock_context = base
                .init_offchain_mock_context(block_producer_config)
                .await?;
            let mem_pool_provider = DefaultMemPoolProvider::new(
                base.rpc_client.clone(),
                base.store.clone(),
                config.mem_pool.mem_block.clone(),
            );
            let mem_pool = {
                let block_producer = RegistryAddress::new(
                    block_producer_config.block_producer.registry_id,
                    block_producer_config
                        .block_producer
                        .address
                        .as_bytes()
                        .to_vec(),
                );
                let account_creator = {
                    match config.block_producer.as_ref().map(|c| &c.wallet_config) {
                        Some(Some(c)) => {
                            log::info!("pool account creator use block producer wallet");
                            let w = Wallet::from_config(c).with_context(|| "pool wallet")?;
                            Some(AccountCreator::create(base.generator.rollup_context(), w)?)
                        }
                        _ if config.node_mode == NodeMode::FullNode => {
                            log::warn!("no wallet config for mem pool account creator");
                            None
                        }
                        _ => None,
                    }
                };
                let args = MemPoolCreateArgs {
                    block_producer,
                    store: base.store.clone(),
                    generator: base.generator.clone(),
                    provider: Box::new(mem_pool_provider),
                    config: config.mem_pool.clone(),
                    node_mode: config.node_mode,
                    dynamic_config_manager: base.dynamic_config_manager.clone(),
                    sync_server: block_sync_server_state.clone(),
                    account_creator,
                };
                Arc::new(Mutex::new(
                    MemPool::create(args)
                        .await
                        .with_context(|| "create mem-pool")?,
                ))
            };
            (Some(mem_pool), opt_wallet, opt_offchain_mock_context)
        }
        None => (None, None, None),
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

    let liveness = Arc::new(Liveness::new(Duration::from_secs(
        config.liveness_duration_secs.unwrap_or(60),
    )));

    // check state db
    {
        let t = Instant::now();
        store.check_state()?;
        log::info!("Check state db done: {}ms", t.elapsed().as_millis());
    }
    let chain = Arc::new(Mutex::new(
        Chain::create(
            rollup_config.clone(),
            &config.chain.rollup_type_script.clone().into(),
            &config.chain,
            store.clone(),
            generator.clone(),
            mem_pool.clone(),
        )
        .with_context(|| "create chain")?,
    ));

    // create chain updater
    let chain_updater = ChainUpdater::new(
        Arc::clone(&chain),
        rpc_client.clone(),
        rollup_context.clone(),
        rollup_type_script.clone(),
    );

    let (block_producer, challenger, test_mode_control, withdrawal_unlocker, cleaner) = match config
        .node_mode
    {
        NodeMode::ReadOnly => (None, None, None, None, None),
        mode => {
            let block_producer_config = config
                .block_producer
                .clone()
                .ok_or_else(|| anyhow!("must provide block producer config in mode: {:?}", mode))?;
            let contracts_dep_manager =
                contracts_dep_manager.ok_or_else(|| anyhow!("must build contracts dep"))?;
            let wallet =
                wallet.ok_or_else(|| anyhow!("wallet must be enabled in mode: {:?}", mode))?;
            let offchain_mock_context = {
                let ctx = offchain_mock_context;
                let msg = "offchain mock require block producer config and wallet in mode: ";
                ctx.ok_or_else(|| anyhow!("{} {:?}", msg, mode))?
            };
            let tests_control = if let NodeMode::Test = config.node_mode {
                Some(TestModeControl::new(rpc_client.clone(), store.clone()))
            } else {
                None
            };

            let unlocker_wallet = match block_producer_config.withdrawal_unlocker_wallet_config {
                Some(ref wallet_config) => {
                    Wallet::from_config(wallet_config).with_context(|| "unlocker wallet")?
                }
                None => {
                    log::info!("[unlock withdrawal] reuse block producer wallet");

                    match block_producer_config.wallet_config {
                        Some(ref c) => Wallet::from_config(c).with_context(|| "unlocker wallet")?,
                        None => bail!("no wallet config for withdrawal unlocker"),
                    }
                }
            };

            let withdrawal_unlocker = FinalizedWithdrawalUnlocker::new(
                rpc_client.clone(),
                ckb_genesis_info.clone(),
                contracts_dep_manager.clone(),
                unlocker_wallet,
                config.debug.clone(),
                block_producer_config.fee_rate,
            );

            let cleaner = Arc::new(Cleaner::new(
                rpc_client.clone(),
                ckb_genesis_info.clone(),
                wallet,
                block_producer_config.fee_rate,
            ));

            let challenger_wallet = match block_producer_config.wallet_config {
                Some(ref c) => Wallet::from_config(c).with_context(|| "challenger wallet")?,
                None => bail!("no wallet config for block producer"),
            };

            // Challenger
            let args = ChallengerNewArgs {
                rollup_context,
                rpc_client: rpc_client.clone(),
                wallet: challenger_wallet,
                config: block_producer_config.clone(),
                debug_config: config.debug.clone(),
                builtin_load_data,
                ckb_genesis_info: ckb_genesis_info.clone(),
                chain: Arc::clone(&chain),
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
                rpc_client: rpc_client.clone(),
                ckb_genesis_info,
                config: block_producer_config,
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

    // Wait for graceful shutdown complete.
    let (shutdown_completed_send, mut shutdown_completed_recv) = mpsc::channel(1);
    // Broadcast shutdown event.
    let (shutdown_event, shutdown_event_recv) = broadcast::channel(1);

    let block_sync_client_p2p_stream_inbox: Arc<std::sync::Mutex<Option<P2PStream>>> =
        Arc::new(std::sync::Mutex::new(None));

    // P2P network.
    let p2p_control_and_handle = if let Some(ref p2p_network_config) = config.p2p_network_config {
        let mut protocols: Vec<ProtocolMeta> = Vec::new();
        match config.node_mode {
            NodeMode::ReadOnly => {
                log::info!("will enable p2p block sync client");
                protocols.push(block_sync_client_protocol(
                    block_sync_client_p2p_stream_inbox.clone(),
                ));
            }
            NodeMode::FullNode | NodeMode::Test => {
                if let Some(ref state) = block_sync_server_state {
                    log::info!("will enable p2p block sync server");
                    protocols.push(block_sync_server_protocol(state.clone()));
                }
            }
        }
        let mut network = P2PNetwork::init(p2p_network_config, protocols).await?;
        let control = network.control().clone();
        let handle = tokio::spawn(async move {
            log::info!("running the p2p network");
            network.run().await;
        });
        Some((control, handle))
    } else {
        None
    };

    // RPC registry
    let polyjuice_sender_recover = {
        let opt_wallet = match config.block_producer.as_ref().map(|c| &c.wallet_config) {
            Some(Some(c)) => {
                log::info!("[tx from zero] use block producer wallet");

                Some(Wallet::from_config(c).with_context(|| "polyjuice sender creator wallet")?)
            }
            _ => {
                log::info!("[tx from zero] no wallet config for polyjuice sender creator");

                None
            }
        };

        PolyjuiceSenderRecover::create(generator.rollup_context(), opt_wallet)?
    };
    let args = RegistryArgs {
        store: store.clone(),
        mem_pool: mem_pool.clone(),
        generator,
        tests_rpc_impl: test_mode_control.map(Box::new),
        rollup_config,
        chain_config: config.chain.to_owned(),
        consensus_config: config.consensus.to_owned(),
        mem_pool_config: config.mem_pool.clone(),
        node_mode: config.node_mode,
        rpc_client: rpc_client.clone(),
        send_tx_rate_limit: config.dynamic_config.rpc_config.send_tx_rate_limit.clone(),
        server_config: config.rpc_server.clone(),
        dynamic_config_manager,
        polyjuice_sender_recover,
        debug_backend_forks: config.debug_backend_forks.clone(),
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

    let bm = (block_producer, mem_pool.clone()); // To keep the next line short.
    let psc_task = if let (Some(block_producer), Some(mem_pool)) = bm {
        let psc_state = ProduceSubmitConfirm::init(Arc::new(PSCContext {
            store: store.clone(),
            block_producer,
            rpc_client: rpc_client.clone(),
            chain: chain.clone(),
            mem_pool,
            local_cells_manager: Mutex::new(LocalCellsManager::default()),
            chain_updater: chain_updater.clone(),
            rollup_type_script: rollup_type_script.clone(),
            psc_config: config.block_producer.as_ref().unwrap().psc_config.clone(),
            block_sync_server_state: block_sync_server_state.clone(),
            liveness: liveness.clone(),
        }))
        .await
        .context("create ProduceSubmitConfirm")?;

        let shutdown_completed_send = shutdown_completed_send.clone();
        let mut shutdown_event_recv = shutdown_event.subscribe();
        Some(tokio::spawn(async move {
            let result = tokio::select! {
                _ = shutdown_event_recv.recv() => return,
                result = psc_state.run() => result,
            };
            if let Err(e) = result {
                log::error!("ProduceSubmitConfirm error: {:#}", e);
            }
            drop(shutdown_completed_send);
        }))
    } else {
        None
    };
    let has_psc_task = psc_task.is_some();
    let psc_task = OptionFuture::from(psc_task);

    let block_sync_task = if config.node_mode == NodeMode::ReadOnly {
        let client = BlockSyncClient {
            store: store.clone(),
            rpc_client: rpc_client.clone(),
            chain: chain.clone(),
            mem_pool,
            chain_updater,
            rollup_type_script: rollup_type_script.clone(),
            p2p_stream_inbox: block_sync_client_p2p_stream_inbox,
            completed_initial_syncing: false,
            liveness: liveness.clone(),
        };
        let shutdown_completed_send = shutdown_completed_send.clone();
        let mut shutdown_event_recv = shutdown_event.subscribe();
        Some(tokio::spawn(async move {
            tokio::select! {
                _ = shutdown_event_recv.recv() => {},
                _ = client.run() => {},
            }
            drop(shutdown_completed_send);
        }))
    } else {
        None
    };
    let has_block_sync_task = block_sync_task.is_some();
    let block_sync_task = OptionFuture::from(block_sync_task);

    let (chain_task_ended_tx, chain_task) = tokio::sync::oneshot::channel::<()>();
    let rt_handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking({
        let shutdown_send = shutdown_completed_send.clone();
        move || {
            rt_handle.block_on(async move {
                use tracing::Instrument;

                let _tx = chain_task_ended_tx;
                let ctx = ChainTaskContext {
                    // chain_updater,
                    challenger,
                    withdrawal_unlocker,
                    cleaner,
                };
                let mut backoff = ExponentialBackoff::new(Duration::from_secs(1));
                let mut chain_task = ChainTask::create(
                    rpc_client,
                    Duration::from_secs(3),
                    ctx,
                    shutdown_send,
                    shutdown_event_recv,
                );

                let mut run_status = ChainTaskRunStatus::default();
                loop {
                    // Exit if shutdown event is received.
                    if chain_task.shutdown_event.try_recv().is_ok() {
                        log::info!("ChainTask existed successfully");
                        return;
                    }

                    let run_span = info_span!("chain_task_run");
                    match chain_task
                        .run(&run_status)
                        .instrument(run_span.clone())
                        .await
                    {
                        Ok(updated_status) => {
                            run_status = updated_status;
                            backoff.reset();

                            let sleep_span =
                                info_span!(parent: &run_span, "chain_task interval sleep");
                            tokio::time::sleep(chain_task.poll_interval)
                                .instrument(sleep_span)
                                .await;
                        }
                        Err(err) if err.is::<RPCRequestError>() => {
                            // Reset status and refresh tip number hash
                            run_status = ChainTaskRunStatus::default();
                            let backoff_sleep = backoff.next_sleep();
                            log::error!(
                                "chain polling loop request error, will retry in {}s: {}",
                                backoff_sleep.as_secs(),
                                err
                            );

                            let sleep_span =
                                info_span!(parent: &run_span, "chain_task backoff sleep");
                            tokio::time::sleep(backoff_sleep)
                                .instrument(sleep_span)
                                .await;
                        }
                        Err(err) => {
                            log::error!("chain polling loop exit unexpected, error: {}", err);
                            break;
                        }
                    }
                }
            });
        }
    });

    let sub_shutdown = shutdown_event.subscribe();
    let rpc_shutdown_send = shutdown_completed_send.clone();
    let rpc_task = spawn(async move {
        if let Err(err) = start_jsonrpc_server(
            rpc_address,
            rpc_registry,
            liveness,
            rpc_shutdown_send,
            sub_shutdown,
        )
        .await
        {
            log::error!("Error running JSONRPC server: {:?}", err);
        }
    });

    tokio::select! {
        _ = sigint_or_sigterm() => {},
        _ = chain_task => {},
        _ = rpc_task => {},
        _ = psc_task, if has_psc_task => {},
        _ = block_sync_task, if has_block_sync_task => {},
    };

    //If any task is out of running, broadcast shutdown event.
    log::info!("send shutdown event");
    if let Err(err) = shutdown_event.send(()) {
        log::error!("Failed to brodcast error message: {:?}", err);
    }
    // Shutdown p2p network.
    if let Some((control, handle)) = p2p_control_and_handle {
        log::info!("closing p2p network");
        let _ = control.close().await;
        let _ = handle.await;
        log::info!("p2p network closed");
    }

    // Make sure all the senders are dropped.
    drop(shutdown_completed_send);

    // When every sender has gone out of scope, the recv call
    // will return with an error. We ignore the error. Just
    // make sure we can hit this line.
    let _ = shutdown_completed_recv.recv().await;
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
    let cell_data = RollupConfig::from_slice(&rollup_config_cell.data)?;
    let eoa_set = rollup_config
        .allowed_eoa_type_hashes()
        .into_iter()
        .map(|th| th.hash())
        .collect::<Vec<_>>();
    let contract_set = rollup_config
        .allowed_contract_type_hashes()
        .into_iter()
        .map(|th| th.hash())
        .collect::<Vec<_>>();
    let unregistered_eoas = cell_data
        .allowed_eoa_type_hashes()
        .into_iter()
        .map(|th| th.hash())
        .filter(|item| !eoa_set.contains(item))
        .collect::<Vec<_>>();
    let unregistered_contracts = cell_data
        .allowed_contract_type_hashes()
        .into_iter()
        .map(|th| th.hash())
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
    let opt_wallet_config = block_producer_config.wallet_config.as_ref();
    if let Some(block_producer_wallet_lock) = opt_wallet_config.map(|c| &c.lock) {
        if zeros == block_producer_wallet_lock.code_hash {
            bail!("[block_producer.wallet.lock.code_hash] shouldn't be zero");
        }

        let challenger_rewards_receiver_lock = &block_producer_config
            .challenger_config
            .rewards_receiver_lock;
        if block_producer_wallet_lock == challenger_rewards_receiver_lock {
            bail!("[block_producer.challenger.rewards_receiver_lock] and [block_producer.wallet.lock] have the same address, which is not recommended");
        }
    }

    Ok(())
}

fn is_l1_query_error(err: &anyhow::Error) -> bool {
    use crate::chain_updater::QueryL1TxError;

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

fn spawn_starvation_detector() {
    tokio::spawn(async move {
        let mut instant = Instant::now();
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let now = Instant::now();
            let late_millis = (now.duration_since(instant).as_millis() as u32).saturating_sub(500);
            if late_millis > 200 {
                log::warn!("starvation detected: {}ms late", late_millis);
            } else if late_millis > 50 {
                log::info!("micro-starvation detected: {}ms late", late_millis);
            }
            instant = now;
        }
    });
}
