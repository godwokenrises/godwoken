use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use ckb_types::prelude::{Builder, Entity};
use gw_common::blake2b::new_blake2b;
use gw_common::builtins::{CKB_SUDT_ACCOUNT_ID, ETH_REGISTRY_ACCOUNT_ID};
use gw_common::{state::State, H256};
use gw_config::{
    ChainConfig, ConsensusConfig, FeeConfig, MemPoolConfig, NodeMode, RPCMethods, RPCRateLimit,
    RPCServerConfig, SyscallCyclesConfig,
};
use gw_dynamic_config::manager::{DynamicConfigManager, DynamicConfigReloadResponse};
use gw_generator::generator::CyclesPool;
use gw_generator::utils::get_tx_type;
use gw_generator::{
    error::TransactionError, sudt::build_l2_sudt_script,
    verification::transaction::TransactionVerifier, ArcSwap, Generator,
};
use gw_jsonrpc_types::godwoken::L2WithdrawalCommittedInfo;
use gw_jsonrpc_types::{
    blockchain::Script,
    ckb_jsonrpc_types::{JsonBytes, Uint32},
    godwoken::{
        BackendInfo, BackendType, EoaScript, EoaScriptType, ErrorTxReceipt, GlobalState, GwScript,
        GwScriptType, L2BlockCommittedInfo, L2BlockStatus, L2BlockView, L2BlockWithStatus,
        L2TransactionStatus, L2TransactionWithStatus, LastL2BlockCommittedInfo, NodeInfo,
        NodeRollupConfig, RegistryAddress, RollupCell, RunResult, TxReceipt, WithdrawalStatus,
        WithdrawalWithStatus,
    },
    test_mode::TestModePayload,
};
use gw_mem_pool::fee::{
    queue::FeeQueue,
    types::{FeeEntry, FeeItem, FeeItemKind, FeeItemSender},
};
use gw_polyjuice_sender_recover::{
    mem_execute_tx_state::MemExecuteTxStateTree, recover::PolyjuiceSenderRecover,
};
use gw_rpc_client::rpc_client::RPCClient;
use gw_store::{
    chain_view::ChainView,
    mem_pool_state::{MemPoolState, MemStore},
    state::state_db::StateContext,
    traits::chain_store::ChainStore,
    CfMemStat, Store,
};
use gw_traits::CodeStore;
use gw_types::offchain::RollupContext;
use gw_types::packed::RawL2Transaction;
use gw_types::{
    bytes::Bytes,
    packed::{self, BlockInfo, Byte32, L2Transaction, RollupConfig, WithdrawalRequestExtra},
    prelude::*,
    U256,
};
use gw_version::Version;
use jsonrpc_v2::{Data, Error as RpcError, MapRouter, Params, Server, Server as JsonrpcServer};
use lru::LruCache;
use once_cell::sync::Lazy;
use pprof::ProfilerGuard;
use prometheus_client::encoding::text::Encode;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use std::{
    convert::{TryFrom, TryInto},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, Mutex};
use tracing::instrument;

use crate::in_queue_request_map::{InQueueRequestHandle, InQueueRequestMap};

static PROFILER_GUARD: Lazy<tokio::sync::Mutex<Option<ProfilerGuard>>> =
    Lazy::new(|| tokio::sync::Mutex::new(None));

// type alias
type RPCServer = Arc<Server<MapRouter>>;
type MemPool = Option<Arc<Mutex<gw_mem_pool::pool::MemPool>>>;
type AccountID = Uint32;
type JsonH256 = ckb_fixed_hash::H256;
type BoxedTestsRPCImpl = Box<dyn TestModeRPC + Send + Sync>;
type GwUint64 = gw_jsonrpc_types::ckb_jsonrpc_types::Uint64;
type GwUint32 = gw_jsonrpc_types::ckb_jsonrpc_types::Uint32;
type RpcNodeMode = gw_jsonrpc_types::godwoken::NodeMode;
type RegistryAddressJsonBytes = JsonBytes;

const HEADER_NOT_FOUND_ERR_CODE: i64 = -32000;
const INVALID_NONCE_ERR_CODE: i64 = -32001;
const BUSY_ERR_CODE: i64 = -32006;
const CUSTODIAN_NOT_ENOUGH_CODE: i64 = -32007;
const INTERNAL_ERROR_ERR_CODE: i64 = -32099;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_AVAILABLE_ERR_CODE: i64 = -32601;
const INVALID_PARAM_ERR_CODE: i64 = -32602;
const RATE_LIMIT_ERR_CODE: i64 = -32603;

type SendTransactionRateLimiter = Mutex<LruCache<u32, Instant>>;

fn rate_limit_err() -> RpcError {
    RpcError::Provided {
        code: RATE_LIMIT_ERR_CODE,
        message: "Rate limit, please wait few seconds and try again",
    }
}

fn header_not_found_err() -> RpcError {
    RpcError::Provided {
        code: HEADER_NOT_FOUND_ERR_CODE,
        message: "header not found",
    }
}

fn mem_pool_is_disabled_err() -> RpcError {
    RpcError::Provided {
        code: METHOD_NOT_AVAILABLE_ERR_CODE,
        message: "mem-pool is disabled",
    }
}

fn invalid_param_err(msg: &'static str) -> RpcError {
    RpcError::Provided {
        code: INVALID_PARAM_ERR_CODE,
        message: msg,
    }
}

#[async_trait]
pub trait TestModeRPC {
    async fn get_global_state(&self) -> Result<GlobalState>;
    async fn produce_block(&self, payload: TestModePayload) -> Result<()>;
}

fn to_h256(v: JsonH256) -> H256 {
    let h: [u8; 32] = v.into();
    h.into()
}

fn to_jsonh256(v: H256) -> JsonH256 {
    let h: [u8; 32] = v.into();
    h.into()
}

pub struct ExecutionTransactionContext {
    mem_pool: MemPool,
    generator: Arc<Generator>,
    store: Store,
    mem_pool_state: Arc<MemPoolState>,
    polyjuice_sender_recover: Arc<PolyjuiceSenderRecover>,
    mem_pool_config: MemPoolConfig,
    metrics: Family<RunResultLabel, Counter>,
}

pub struct SubmitTransactionContext {
    in_queue_request_map: Option<Arc<InQueueRequestMap>>,
    submit_tx: mpsc::Sender<(InQueueRequestHandle, Request)>,
    mem_pool_state: Arc<MemPoolState>,
    rate_limiter: Option<SendTransactionRateLimiter>,
    rate_limit_config: Option<RPCRateLimit>,
    polyjuice_sender_recover: Arc<PolyjuiceSenderRecover>,
}

pub struct RegistryArgs<T> {
    pub store: Store,
    pub mem_pool: MemPool,
    pub generator: Arc<Generator>,
    pub tests_rpc_impl: Option<Box<T>>,
    pub rollup_config: RollupConfig,
    pub mem_pool_config: MemPoolConfig,
    pub node_mode: NodeMode,
    pub rpc_client: RPCClient,
    pub send_tx_rate_limit: Option<RPCRateLimit>,
    pub server_config: RPCServerConfig,
    pub chain_config: ChainConfig,
    pub consensus_config: ConsensusConfig,
    pub dynamic_config_manager: Arc<ArcSwap<DynamicConfigManager>>,
    pub polyjuice_sender_recover: PolyjuiceSenderRecover,
}

// Label for the execute_transactions metric.
#[derive(Hash, Clone, Eq, PartialEq)]
struct RunResultLabel {
    exit_code: i8,
}

// Manual impl because i8 does not implement Encode.
impl Encode for RunResultLabel {
    fn encode(&self, writer: &mut dyn std::io::Write) -> Result<(), std::io::Error> {
        write!(writer, "exit_code=\"{}\"", self.exit_code)
    }
}

pub struct Registry {
    generator: Arc<Generator>,
    mem_pool: MemPool,
    store: Store,
    tests_rpc_impl: Option<Arc<BoxedTestsRPCImpl>>,
    rollup_config: RollupConfig,
    mem_pool_config: MemPoolConfig,
    backend_info: Vec<BackendInfo>,
    node_mode: NodeMode,
    submit_tx: mpsc::Sender<(InQueueRequestHandle, Request)>,
    rpc_client: RPCClient,
    send_tx_rate_limit: Option<RPCRateLimit>,
    server_config: RPCServerConfig,
    chain_config: ChainConfig,
    consensus_config: ConsensusConfig,
    dynamic_config_manager: Arc<ArcSwap<DynamicConfigManager>>,
    mem_pool_state: Arc<MemPoolState>,
    in_queue_request_map: Option<Arc<InQueueRequestMap>>,
    polyjuice_sender_recover: Arc<PolyjuiceSenderRecover>,
    execute_transaction_metrics: Family<RunResultLabel, Counter>,
}

impl Registry {
    pub async fn create<T>(args: RegistryArgs<T>) -> Self
    where
        T: TestModeRPC + Send + Sync + 'static,
    {
        let RegistryArgs {
            generator,
            mem_pool,
            store,
            tests_rpc_impl,
            rollup_config,
            mem_pool_config,
            node_mode,
            rpc_client,
            send_tx_rate_limit,
            server_config,
            chain_config,
            consensus_config,
            dynamic_config_manager,
            polyjuice_sender_recover,
        } = args;

        let backend_info = get_backend_info(generator.clone());

        let mem_pool_state = match mem_pool.as_ref() {
            Some(pool) => {
                let mem_pool = pool.lock().await;
                mem_pool.mem_pool_state()
            }
            None => Arc::new(MemPoolState::new(
                Arc::new(MemStore::new(store.get_snapshot())),
                true,
            )),
        };
        let in_queue_request_map = if matches!(node_mode, NodeMode::FullNode | NodeMode::Test) {
            Some(Arc::new(InQueueRequestMap::create_and_register_metrics()))
        } else {
            None
        };
        let (submit_tx, submit_rx) = mpsc::channel(RequestSubmitter::MAX_CHANNEL_SIZE);
        let polyjuice_sender_recover = Arc::new(polyjuice_sender_recover);
        if let Some(mem_pool) = mem_pool.as_ref().to_owned() {
            let submitter = RequestSubmitter {
                mem_pool: Arc::clone(mem_pool),
                submit_rx,
                queue: FeeQueue::new(),
                dynamic_config_manager: dynamic_config_manager.clone(),
                generator: generator.clone(),
                mem_pool_state: mem_pool_state.clone(),
                store: store.clone(),
                polyjuice_sender_recover: Arc::clone(&polyjuice_sender_recover),
                mem_pool_config: mem_pool_config.clone(),
            };
            tokio::spawn(submitter.in_background());
        }

        let execute_transaction_metrics = Family::default();
        gw_metrics::REGISTRY
            .write()
            .unwrap()
            .sub_registry_with_prefix("rpc")
            .register(
                "execute_transactions",
                "Number of execute_transaction requests",
                Box::new(execute_transaction_metrics.clone()),
            );

        Self {
            mem_pool,
            store,
            generator,
            tests_rpc_impl: tests_rpc_impl
                .map(|r| Arc::new(r as Box<dyn TestModeRPC + Sync + Send + 'static>)),
            rollup_config,
            mem_pool_config,
            backend_info,
            node_mode,
            submit_tx,
            rpc_client,
            send_tx_rate_limit,
            server_config,
            chain_config,
            consensus_config,
            dynamic_config_manager,
            mem_pool_state,
            in_queue_request_map,
            polyjuice_sender_recover,
            execute_transaction_metrics,
        }
    }

    pub fn build_rpc_server(self) -> Result<RPCServer> {
        let mut server = JsonrpcServer::new();

        let send_transaction_rate_limiter: Option<SendTransactionRateLimiter> = self
            .send_tx_rate_limit
            .as_ref()
            .map(|send_tx_rate_limit| Mutex::new(lru::LruCache::new(send_tx_rate_limit.lru_size)));

        server = server
            .with_data(Data::new(ExecutionTransactionContext {
                mem_pool: self.mem_pool.clone(),
                generator: self.generator.clone(),
                store: self.store.clone(),
                mem_pool_state: self.mem_pool_state.clone(),
                polyjuice_sender_recover: self.polyjuice_sender_recover.clone(),
                mem_pool_config: self.mem_pool_config.clone(),
                metrics: self.execute_transaction_metrics,
            }))
            .with_data(Data::new(SubmitTransactionContext {
                in_queue_request_map: self.in_queue_request_map.clone(),
                submit_tx: self.submit_tx.clone(),
                mem_pool_state: self.mem_pool_state.clone(),
                rate_limiter: send_transaction_rate_limiter,
                rate_limit_config: self.send_tx_rate_limit,
                polyjuice_sender_recover: self.polyjuice_sender_recover.clone(),
            }))
            .with_data(Data::new(self.mem_pool.clone()))
            .with_data(Data(self.generator.clone()))
            .with_data(Data::new(self.store))
            .with_data(Data::new(self.rollup_config))
            .with_data(Data::new(self.mem_pool_config))
            .with_data(Data::new(self.backend_info))
            .with_data(Data::new(self.rpc_client))
            .with_data(Data::new(self.dynamic_config_manager.clone()))
            .with_data(Data::new(self.mem_pool_state))
            .with_data(Data::new(self.chain_config))
            .with_data(Data::new(self.consensus_config))
            .with_data(Data::new(self.node_mode))
            .with_data(Data::new(self.in_queue_request_map))
            .with_data(Data::new(self.submit_tx))
            .with_method("gw_ping", ping)
            .with_method("gw_get_tip_block_hash", get_tip_block_hash)
            .with_method("gw_get_block_hash", get_block_hash)
            .with_method("gw_get_block", get_block)
            .with_method("gw_get_block_by_number", get_block_by_number)
            .with_method("gw_get_block_committed_info", get_block_committed_info)
            .with_method("gw_get_balance", get_balance)
            .with_method("gw_get_storage_at", get_storage_at)
            .with_method(
                "gw_get_account_id_by_script_hash",
                get_account_id_by_script_hash,
            )
            .with_method("gw_get_nonce", get_nonce)
            .with_method("gw_get_script", get_script)
            .with_method("gw_get_script_hash", get_script_hash)
            .with_method(
                "gw_get_script_hash_by_registry_address",
                get_script_hash_by_registry_address,
            )
            .with_method(
                "gw_get_registry_address_by_script_hash",
                get_registry_address_by_script_hash,
            )
            .with_method("gw_get_data", get_data)
            .with_method("gw_get_transaction", get_transaction)
            .with_method("gw_get_transaction_receipt", get_transaction_receipt)
            .with_method("gw_get_withdrawal", get_withdrawal)
            .with_method("gw_get_pending_tx_hashes", get_pending_tx_hashes)
            .with_method("gw_execute_l2transaction", execute_l2transaction)
            .with_method("gw_execute_raw_l2transaction", execute_raw_l2transaction)
            .with_method(
                "gw_compute_l2_sudt_script_hash",
                compute_l2_sudt_script_hash,
            )
            .with_method("gw_get_fee_config", get_fee_config)
            .with_method("gw_get_mem_pool_state_root", get_mem_pool_state_root)
            .with_method("gw_get_mem_pool_state_ready", get_mem_pool_state_ready)
            .with_method("gw_get_node_info", get_node_info)
            .with_method("gw_reload_config", reload_config)
            .with_method("gw_get_last_submitted_info", get_last_submitted_info);

        if self.node_mode != NodeMode::ReadOnly {
            server = server
                .with_method("gw_submit_l2transaction", submit_l2transaction)
                .with_method("gw_submit_withdrawal_request", submit_withdrawal_request)
                .with_method("gw_is_request_in_queue", is_request_in_queue);
        }

        // Tests
        if let Some(tests_rpc_impl) = self.tests_rpc_impl {
            server = server
                .with_data(Data(Arc::clone(&tests_rpc_impl)))
                .with_method("tests_produce_block", tests_produce_block)
                .with_method("tests_get_global_state", tests_get_global_state);
        }

        for enabled in self.server_config.enable_methods.iter() {
            match enabled {
                RPCMethods::PProf => {
                    server = server
                        .with_method("gw_start_profiler", start_profiler)
                        .with_method("gw_report_pprof", report_pprof);
                }
                RPCMethods::Test => {
                    server = server
                        // .with_method("gw_dump_mem_block", dump_mem_block)
                        .with_method("gw_get_rocksdb_mem_stats", get_rocksdb_memory_stats)
                        .with_method("gw_dump_jemalloc_profiling", dump_jemalloc_profiling)
                }
            }
        }

        Ok(server.finish())
    }
}

#[derive(Clone)]
pub(crate) enum Request {
    Tx(L2Transaction),
    Withdrawal(WithdrawalRequestExtra),
}

impl Request {
    fn kind(&self) -> &'static str {
        match self {
            Request::Tx(_) => "tx",
            Request::Withdrawal(_) => "withdrawal",
        }
    }

    fn hash(&self) -> ckb_types::H256 {
        match self {
            Request::Tx(tx) => ckb_types::H256(tx.hash()),
            Request::Withdrawal(withdrawal) => ckb_types::H256(withdrawal.hash()),
        }
    }
}

struct RequestSubmitter {
    mem_pool: Arc<Mutex<gw_mem_pool::pool::MemPool>>,
    submit_rx: mpsc::Receiver<(InQueueRequestHandle, Request)>,
    queue: FeeQueue<InQueueRequestHandle>,
    dynamic_config_manager: Arc<ArcSwap<DynamicConfigManager>>,
    generator: Arc<Generator>,
    mem_pool_state: Arc<MemPoolState>,
    store: Store,
    polyjuice_sender_recover: Arc<PolyjuiceSenderRecover>,
    mem_pool_config: MemPoolConfig,
}

#[instrument(skip_all, fields(req_kind = req.kind()))]
fn req_to_entry(
    fee_config: &FeeConfig,
    generator: Arc<Generator>,
    req: Request,
    state: &(impl State + CodeStore),
    order: usize,
) -> Result<FeeEntry> {
    match req {
        Request::Tx(tx) => {
            let receiver: u32 = tx.raw().to_id().unpack();
            let script_hash = state.get_script_hash(receiver)?;
            let backend_type = generator
                .load_backend(0, state, &script_hash)
                .ok_or_else(|| anyhow!("can't find backend for receiver: {}", receiver))?
                .backend_type;
            FeeEntry::from_tx(tx, fee_config, backend_type, order)
        }
        Request::Withdrawal(withdraw) => {
            let script_hash = withdraw.raw().account_script_hash().unpack();
            let sender = state
                .get_account_id_by_script_hash(&script_hash)?
                .ok_or_else(|| {
                    anyhow!(
                        "can't find id by script hash {}",
                        withdraw.raw().account_script_hash()
                    )
                })?;
            FeeEntry::from_withdrawal(withdraw, sender, fee_config, order)
        }
    }
}

impl RequestSubmitter {
    const MAX_CHANNEL_SIZE: usize = 10000;
    const MAX_BATCH_SIZE: usize = 20;
    const INTERVAL_MS: Duration = Duration::from_millis(100);

    async fn in_background(mut self) {
        // First mem pool reinject txs
        {
            let db = self.store.begin_transaction();
            let mut mem_pool = self.mem_pool.lock().await;

            log::info!(
                "reinject mem block txs {}",
                mem_pool.pending_restored_tx_hashes().len()
            );

            // Use unlimit to ensure all exists mem pool transactions are included
            let mut org_cycles_pool = mem_pool.cycles_pool().clone();
            *mem_pool.cycles_pool_mut() =
                CyclesPool::new(u64::MAX, SyscallCyclesConfig::all_zero());

            while let Some(hash) = mem_pool.pending_restored_tx_hashes().pop_front() {
                match db.get_mem_pool_transaction(&hash) {
                    Ok(Some(tx)) => {
                        if let Err(err) = mem_pool.push_transaction(tx) {
                            log::error!("reinject mem block tx {} failed {}", hash.pack(), err);
                        }
                    }
                    Ok(None) => {
                        log::error!("reinject mem block tx {} not found", hash.pack());
                    }
                    Err(err) => {
                        log::error!("reinject mem block tx {} err {}", hash.pack(), err);
                    }
                }
            }

            // Update remained block cycles
            org_cycles_pool.consume_cycles(mem_pool.cycles_pool().cycles_used());
            *mem_pool.cycles_pool_mut() = org_cycles_pool;
        }

        loop {
            // check mem block empty slots
            loop {
                let dynamic_config_manager = self.dynamic_config_manager.load();
                let fee_config = dynamic_config_manager.get_fee_config();

                log::debug!("[Mem-pool background job] check mem-pool acquire mem_pool",);
                let t = Instant::now();
                let mem_pool = self.mem_pool.lock().await;
                log::debug!(
                    "[Mem-pool background job] check-mem-pool unlock mem_pool {}ms",
                    t.elapsed().as_millis()
                );
                // continue to batch process if we have enough mem block slots
                if !mem_pool.is_mem_txs_full(Self::MAX_BATCH_SIZE)
                    && mem_pool.cycles_pool().available_cycles()
                        >= fee_config.minimal_tx_cycles_limit()
                {
                    break;
                }
                drop(mem_pool);
                // sleep and try again
                tokio::time::sleep(Self::INTERVAL_MS).await;
            }

            // mem-pool can process more txs
            let queue = &mut self.queue;

            // wait next tx if queue is empty
            if queue.is_empty() {
                // blocking current task until we receive a tx
                let (handle, req) = match self.submit_rx.recv().await {
                    Some(req) => req,
                    None => {
                        log::error!("rpc submit tx is closed");
                        return;
                    }
                };
                let snap = self.mem_pool_state.load();
                let state = snap.state().expect("get mem state");
                let kind = req.kind();
                let hash = req.hash();
                let dynamic_config_manager = self.dynamic_config_manager.load();
                let fee_config = dynamic_config_manager.get_fee_config();
                match req_to_entry(fee_config, self.generator.clone(), req, &state, queue.len()) {
                    Ok(entry) => {
                        if entry.cycles_limit > self.mem_pool_config.mem_block.max_cycles_limit {
                            log::info!(
                                "req kind {} hash {} exceeded mem block max cycles limit, drop it",
                                kind,
                                hash,
                            );
                        } else {
                            queue.add(entry, handle);
                        }
                    }
                    Err(err) => {
                        log::error!(
                            "Failed to convert req to entry kind: {}, hash: {}, err: {}",
                            kind,
                            hash,
                            err
                        );
                    }
                }
            }

            // push txs to fee priority queue
            let snap = self.mem_pool_state.load();
            let state = snap.state().expect("get mem state");
            while let Ok((handle, req)) = self.submit_rx.try_recv() {
                let kind = req.kind();
                let hash = req.hash();
                let dynamic_config_manager = self.dynamic_config_manager.load();
                let fee_config = dynamic_config_manager.get_fee_config();
                match req_to_entry(fee_config, self.generator.clone(), req, &state, queue.len()) {
                    Ok(entry) => {
                        if entry.cycles_limit > self.mem_pool_config.mem_block.max_cycles_limit {
                            log::info!(
                                "req kind {} hash {} exceeded mem block max cycles limit, drop it",
                                kind,
                                hash,
                            );
                        } else {
                            queue.add(entry, handle);
                        }
                    }
                    Err(err) => {
                        log::error!(
                            "Failed to convert req to entry kind: {}, hash: {}, err: {}",
                            kind,
                            hash,
                            err
                        );
                    }
                }
            }

            // fetch items from PQ
            let items = match queue.fetch(&state, Self::MAX_BATCH_SIZE) {
                Ok(items) => items,
                Err(err) => {
                    log::error!(
                        "Fetch items({}) from queue({}) error: {}",
                        Self::MAX_BATCH_SIZE,
                        queue.len(),
                        err
                    );
                    continue;
                }
            };

            if !items.is_empty() {
                // recover accounts for polyjuice tx from id zero
                let eth_recover = &self.polyjuice_sender_recover.eth;
                let txs_from_zero = items
                    .iter()
                    .filter_map(|(entry, _handle)| match entry.item {
                        FeeItem::Tx(ref tx)
                            if matches!(entry.sender, FeeItemSender::PendingCreate(_)) =>
                        {
                            Some(tx)
                        }
                        _ => None,
                    });
                let recovered_senders = eth_recover.recover_sender_accounts(txs_from_zero, &state);

                log::debug!("[Mem-pool background job] acquire mem_pool",);
                let t = Instant::now();
                let mut mem_pool = self.mem_pool.lock().await;
                log::debug!(
                    "[Mem-pool background job] unlock mem_pool {}ms",
                    t.elapsed().as_millis()
                );

                if let Err(err) = match recovered_senders.build_create_tx(eth_recover, &state) {
                    Ok(Some(create_accounts_tx)) => mem_pool.push_transaction(create_accounts_tx),
                    Ok(None) => Ok(()),
                    Err(err) => Err(err),
                } {
                    if let Some(TransactionError::InsufficientPoolCycles { .. }) =
                        err.downcast_ref::<TransactionError>()
                    {
                        log::info!("[tx from zero] mem block cycles limit reached, retry later");

                        for (entry, handle) in items {
                            queue.add(entry, handle);
                        }
                        continue;
                    }

                    log::error!("[tx from zero] create account {}", err);
                }

                let snap = self.mem_pool_state.load();
                let state = snap.state().expect("get mem state");
                let mut block_cycles_limit_reached = false;

                for (entry, handle) in items {
                    if let FeeItemKind::Tx = entry.item.kind() {
                        if !block_cycles_limit_reached
                            && entry.cycles_limit > mem_pool.cycles_pool().available_cycles()
                        {
                            let hash: Byte32 = entry.item.hash().pack();
                            log::info!("mem block cycles limit reached for tx {}", hash);

                            block_cycles_limit_reached = true;
                        }

                        if block_cycles_limit_reached {
                            queue.add(entry, handle);
                            continue;
                        }
                    }

                    let maybe_ok = match entry.item.clone() {
                        FeeItem::Tx(tx)
                            if matches!(entry.sender, FeeItemSender::PendingCreate(_)) =>
                        {
                            let sig: Bytes = tx.signature().unpack();
                            let sender_id = match recovered_senders.get_account_id(&sig, &state) {
                                Ok(id) => id,
                                Err(err) => {
                                    log::info!("[from tx zero] {:x} {}", tx.hash().pack(), err);
                                    continue;
                                }
                            };

                            let org_hash = tx.hash();
                            let raw_tx = tx.raw().as_builder().from_id(sender_id.pack()).build();
                            let tx = tx.as_builder().raw(raw_tx).build();
                            log::info!(
                                "[from tx zero] update tx {:x} from id to {}, hash to {:x}",
                                org_hash.pack(),
                                sender_id,
                                tx.hash().pack()
                            );

                            mem_pool.push_transaction(tx)
                        }
                        FeeItem::Tx(tx) => mem_pool.push_transaction(tx),
                        FeeItem::Withdrawal(withdrawal) => {
                            mem_pool.push_withdrawal_request(withdrawal).await
                        }
                    };

                    if let Err(err) = maybe_ok {
                        let hash: Byte32 = entry.item.hash().pack();

                        if let Some(TransactionError::InsufficientPoolCycles { .. }) =
                            err.downcast_ref::<TransactionError>()
                        {
                            log::info!("mem block cycles limit reached for tx {}", hash);

                            block_cycles_limit_reached = true;
                            queue.add(entry, handle);

                            continue;
                        }

                        log::info!("push {:?} {} failed {}", entry.item.kind(), hash, err);
                    }
                }

                if block_cycles_limit_reached {
                    drop(mem_pool);
                    tokio::time::sleep(Self::INTERVAL_MS).await;
                }
            }
        }
    }
}

async fn ping() -> Result<String> {
    Ok("pong".to_string())
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
enum GetTxParams {
    Default((JsonH256,)),
    WithVerbose((JsonH256, u8)),
}

enum GetTxVerbose {
    TxWithStatus = 0,
    OnlyStatus = 1,
}

impl TryFrom<u8> for GetTxVerbose {
    type Error = u8;
    fn try_from(n: u8) -> Result<Self, u8> {
        let verbose = match n {
            0 => Self::TxWithStatus,
            1 => Self::OnlyStatus,
            _ => {
                return Err(n);
            }
        };
        Ok(verbose)
    }
}

async fn get_transaction(
    Params(param): Params<GetTxParams>,
    store: Data<Store>,
    in_queue_request_map: Data<Option<Arc<InQueueRequestMap>>>,
) -> Result<Option<L2TransactionWithStatus>, RpcError> {
    let (tx_hash, verbose) = match param {
        GetTxParams::Default((tx_hash,)) => (to_h256(tx_hash), GetTxVerbose::TxWithStatus),
        GetTxParams::WithVerbose((tx_hash, verbose)) => {
            let verbose = verbose
                .try_into()
                .map_err(|_err| invalid_param_err("invalid verbose param"))?;
            (to_h256(tx_hash), verbose)
        }
    };

    if let Some(tx) = in_queue_request_map
        .as_deref()
        .and_then(|m| m.get_transaction(&tx_hash))
    {
        return Ok(Some(L2TransactionWithStatus {
            transaction: matches!(verbose, GetTxVerbose::TxWithStatus).then(|| tx.into()),
            status: L2TransactionStatus::Pending,
        }));
    }
    let db = store.get_snapshot();
    let tx_opt;
    let status;
    match db.get_transaction_info(&tx_hash)? {
        Some(tx_info) => {
            tx_opt = db.get_transaction_by_key(&tx_info.key())?;
            status = L2TransactionStatus::Committed;
        }
        None => {
            tx_opt = db.get_mem_pool_transaction(&tx_hash)?;
            status = L2TransactionStatus::Pending;
        }
    };

    Ok(tx_opt.map(|tx| match verbose {
        GetTxVerbose::OnlyStatus => L2TransactionWithStatus {
            transaction: None,
            status,
        },
        GetTxVerbose::TxWithStatus => L2TransactionWithStatus {
            transaction: Some(tx.into()),
            status,
        },
    }))
}

async fn get_pending_tx_hashes(store: Data<Store>) -> Result<Vec<JsonH256>, RpcError> {
    let snap = store.get_snapshot();
    let tx_hashes = snap
        .iter_mem_pool_transactions()
        .map(|hash| JsonH256::from_slice(&hash))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(tx_hashes)
}

async fn is_request_in_queue(
    Params((hash,)): Params<(JsonH256,)>,
    in_queue_request_map: Data<Option<Arc<InQueueRequestMap>>>,
) -> Result<bool, RpcError> {
    let hash = to_h256(hash);

    Ok(in_queue_request_map
        .as_deref()
        .map_or(false, |m| m.contains(&hash)))
}

async fn get_block_committed_info(
    Params((block_hash,)): Params<(JsonH256,)>,
    rpc_client: Data<RPCClient>,
    store: Data<Store>,
) -> Result<Option<L2BlockCommittedInfo>> {
    if let Some(number) = store.get_block_number(&to_h256(block_hash))? {
        if let Some(transaction_hash) = store.get_block_submit_tx_hash(number) {
            let opt_block_hash = rpc_client
                .ckb
                .get_transaction_block_hash(transaction_hash)
                .await?;
            if let Some(block_hash) = opt_block_hash {
                let number = rpc_client
                    .get_header(block_hash.into())
                    .await?
                    .context("get block header")?
                    .inner
                    .number;
                Ok(Some(L2BlockCommittedInfo {
                    number,
                    block_hash: block_hash.into(),
                    transaction_hash: to_jsonh256(transaction_hash),
                }))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    } else {
        Ok(None)
    }
}

async fn get_block(
    Params((block_hash,)): Params<(JsonH256,)>,
    store: Data<Store>,
    rollup_config: Data<RollupConfig>,
) -> Result<Option<L2BlockWithStatus>> {
    let block_hash = to_h256(block_hash);
    let db = store.begin_transaction();
    let block = match db.get_block(&block_hash)? {
        Some(block) => block,
        None => return Ok(None),
    };

    // check block status
    let mut status = L2BlockStatus::Unfinalized;
    if !db.reverted_block_smt()?.get(&block_hash)?.is_zero() {
        // block is reverted
        status = L2BlockStatus::Reverted;
    } else {
        // return None if block is not on the main chain
        if db.block_smt()?.get(&block.smt_key().into())? != block_hash {
            return Ok(None);
        }

        // block is on main chain
        let last_confirmed_block_number = db
            .get_last_confirmed_block_number_hash()
            .map(|nh| nh.number().unpack())
            .unwrap_or(0);
        let block_number = block.raw().number().unpack();
        if last_confirmed_block_number >= block_number + rollup_config.finality_blocks().unpack() {
            status = L2BlockStatus::Finalized;
        }
    }

    Ok(Some(L2BlockWithStatus {
        block: block.into(),
        status,
    }))
}

async fn get_block_by_number(
    Params((block_number,)): Params<(gw_jsonrpc_types::ckb_jsonrpc_types::Uint64,)>,
    mem_pool_state: Data<Arc<MemPoolState>>,
) -> Result<Option<L2BlockView>> {
    let block_number = block_number.value();
    let snap = mem_pool_state.load();
    let block_hash = match snap.get_block_hash_by_number(block_number)? {
        Some(hash) => hash,
        None => return Ok(None),
    };
    let block_opt = snap.get_block(&block_hash)?.map(|block| {
        let block_view: L2BlockView = block.into();
        block_view
    });
    Ok(block_opt)
}

async fn get_block_hash(
    Params((block_number,)): Params<(gw_jsonrpc_types::ckb_jsonrpc_types::Uint64,)>,
    mem_pool_state: Data<Arc<MemPoolState>>,
) -> Result<Option<JsonH256>> {
    let block_number = block_number.value();
    let db = mem_pool_state.load();
    let hash_opt = db.get_block_hash_by_number(block_number)?.map(to_jsonh256);
    Ok(hash_opt)
}

async fn get_tip_block_hash(mem_pool_state: Data<Arc<MemPoolState>>) -> Result<JsonH256> {
    let tip_block_hash = mem_pool_state.load().get_last_valid_tip_block_hash()?;
    Ok(to_jsonh256(tip_block_hash))
}

async fn get_transaction_receipt(
    Params((tx_hash,)): Params<(JsonH256,)>,
    store: Data<Store>,
) -> Result<Option<TxReceipt>> {
    let tx_hash = to_h256(tx_hash);
    let db = store.get_snapshot();
    // search from db
    if let Some(receipt) = db.get_transaction_receipt(&tx_hash)?.map(|receipt| {
        let receipt: TxReceipt = receipt.into();
        receipt
    }) {
        return Ok(Some(receipt));
    }
    // search from mem pool
    Ok(db
        .get_mem_pool_transaction_receipt(&tx_hash)?
        .map(Into::into))
}

fn verify_sender_balance<S: State + CodeStore>(
    ctx: &RollupContext,
    state: &S,
    raw_tx: &RawL2Transaction,
) -> Result<()> {
    use gw_generator::typed_transaction::types::TypedRawTransaction;

    let sender_id: u32 = raw_tx.from_id().unpack();
    // verify balance
    let sender_script_hash = state.get_script_hash(sender_id)?;
    let sender_address = state
        .get_registry_address_by_script_hash(ETH_REGISTRY_ACCOUNT_ID, &sender_script_hash)?
        .ok_or_else(|| anyhow!("Can't find address for sender: {}", sender_id))?;
    // get balance
    let balance = state.get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &sender_address)?;
    let tx_type = get_tx_type(ctx, state, raw_tx)?;
    let typed_tx = TypedRawTransaction::from_tx(raw_tx.to_owned(), tx_type)
        .ok_or_else(|| anyhow!("Unknown type of transaction {:?}", tx_type))?;
    // reject txs has no cost, these transaction can only be execute without modify state tree
    let tx_cost = typed_tx
        .cost()
        .map(Into::into)
        .ok_or(TransactionError::NoCost)?;
    if balance < tx_cost {
        return Err(TransactionError::InsufficientBalance.into());
    }
    Ok(())
}

#[instrument(skip_all)]
async fn execute_l2transaction(
    Params((l2tx,)): Params<(JsonBytes,)>,
    ctx: Data<ExecutionTransactionContext>,
) -> Result<RunResult, RpcError> {
    if ctx.mem_pool.is_none() {
        return Err(mem_pool_is_disabled_err());
    }

    let l2tx_bytes = l2tx.into_bytes();
    let tx = packed::L2Transaction::from_slice(&l2tx_bytes)?;

    let raw_block = ctx.store.get_snapshot().get_last_valid_tip_block()?.raw();
    let block_producer = raw_block.block_producer();
    let timestamp = raw_block.timestamp();
    let number = {
        let number: u64 = raw_block.number().unpack();
        number.saturating_add(1)
    };

    let block_info = BlockInfo::new_builder()
        .block_producer(block_producer)
        .timestamp(timestamp)
        .number(number.pack())
        .build();

    let tx_hash = tx.hash();

    // check sender's balance
    // NOTE: for tx from id 0, it's balance will be verified after mock account
    let from_id: u32 = tx.raw().from_id().unpack();
    if 0 != from_id {
        let snap = ctx.mem_pool_state.load();
        let state = snap.state()?;
        if let Err(err) = verify_sender_balance(ctx.generator.rollup_context(), &state, &tx.raw()) {
            return Err(RpcError::Full {
                code: INVALID_REQUEST,
                message: format!("check balance err: {}", err),
                data: None,
            });
        }
    }

    let metrics = ctx.metrics.clone();
    let mut run_result = tokio::task::spawn_blocking(move || {
        let db = ctx.store.get_snapshot();
        let tip_block_hash = db.get_last_valid_tip_block_hash()?;
        let chain_view = ChainView::new(&db, tip_block_hash);
        let snap = ctx.mem_pool_state.load();
        let mem_state = snap.state()?;
        let mut state = MemExecuteTxStateTree::new(mem_state);
        let mut cycles_pool = CyclesPool::new(
            ctx.mem_pool_config.mem_block.max_cycles_limit,
            ctx.mem_pool_config.mem_block.syscall_cycles.clone(),
        );

        // Mock sender account if not exists
        let eth_recover = &ctx.polyjuice_sender_recover.eth;
        let tx = eth_recover.mock_sender_if_not_exists(tx, &mut state)?;
        if 0 == from_id {
            verify_sender_balance(ctx.generator.rollup_context(), &state, &tx.raw())
                .map_err(|err| anyhow!("check balance err: {}", err))?;
        }

        // tx basic verification
        let polyjuice_creator_id = ctx.generator.get_polyjuice_creator_id(&state)?;
        TransactionVerifier::new(&state, ctx.generator.rollup_context(), polyjuice_creator_id)
            .verify(&tx)?;
        // verify tx signature
        ctx.generator.check_transaction_signature(&state, &tx)?;
        // execute tx
        let raw_tx = tx.raw();
        let run_result = ctx.generator.unchecked_execute_transaction(
            &chain_view,
            &state,
            &block_info,
            &raw_tx,
            100000000,
            Some(&mut cycles_pool),
        )?;

        Result::<_, anyhow::Error>::Ok(run_result)
    })
    .await??;

    metrics
        .get_or_create(&RunResultLabel {
            exit_code: run_result.exit_code,
        })
        .inc();

    if run_result.exit_code != 0 {
        let receipt = gw_types::offchain::ErrorTxReceipt {
            tx_hash: tx_hash.into(),
            block_number: number,
            return_data: run_result.return_data,
            last_log: run_result.write.logs.pop(),
            exit_code: run_result.exit_code,
        };

        return Err(RpcError::Full {
            code: INVALID_REQUEST,
            message: TransactionError::InvalidExitCode(run_result.exit_code).to_string(),
            data: Some(Box::new(ErrorTxReceipt::from(receipt))),
        });
    }

    Ok(run_result.into())
}

// raw_l2tx, block_number
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
enum ExecuteRawL2TransactionParams {
    Tip((JsonBytes,)),
    Number((JsonBytes, Option<GwUint64>)),
    PolyjuiceFromIdZero((JsonBytes, Option<GwUint64>, RegistryAddressJsonBytes)),
}

#[instrument(skip_all)]
async fn execute_raw_l2transaction(
    Params(params): Params<ExecuteRawL2TransactionParams>,
    mem_pool_config: Data<MemPoolConfig>,
    ctx: Data<ExecutionTransactionContext>,
) -> Result<RunResult, RpcError> {
    let (raw_l2tx, block_number_opt, registry_address_opt) = match params {
        ExecuteRawL2TransactionParams::Tip(p) => (p.0, None, None),
        ExecuteRawL2TransactionParams::Number(p) => (p.0, p.1, None),
        ExecuteRawL2TransactionParams::PolyjuiceFromIdZero(p) => (p.0, p.1, Some(p.2)),
    };
    let block_number_opt = block_number_opt.map(|n| n.value());
    let registry_address_opt = registry_address_opt
        .map(|json_bytes| {
            gw_common::registry_address::RegistryAddress::from_slice(json_bytes.as_bytes())
                .ok_or_else(|| invalid_param_err("Invalid registry address"))
        })
        .transpose()?;

    let raw_l2tx_bytes = raw_l2tx.into_bytes();
    let raw_l2tx = packed::RawL2Transaction::from_slice(&raw_l2tx_bytes)?;

    let db = ctx.store.begin_transaction();

    let mem_state_snap = ctx.mem_pool_state.load();

    let block_info = match block_number_opt {
        Some(block_number) => {
            let block_hash = match db.get_block_hash_by_number(block_number)? {
                Some(block_hash) => block_hash,
                None => return Err(header_not_found_err()),
            };
            let raw_block = match ctx.store.get_block(&block_hash)? {
                Some(block) => block.raw(),
                None => return Err(header_not_found_err()),
            };
            let block_producer = raw_block.block_producer();
            let timestamp = raw_block.timestamp();
            let number: u64 = raw_block.number().unpack();

            BlockInfo::new_builder()
                .block_producer(block_producer)
                .timestamp(timestamp)
                .number(number.pack())
                .build()
        }
        None => mem_state_snap
            .get_mem_pool_block_info()?
            .expect("get mem pool block info"),
    };

    let execute_l2tx_max_cycles = mem_pool_config.execute_l2tx_max_cycles;
    let tx_hash: H256 = raw_l2tx.hash().into();
    let block_number: u64 = block_info.number().unpack();
    let mut cycles_pool = CyclesPool::new(
        ctx.mem_pool_config.mem_block.max_cycles_limit,
        ctx.mem_pool_config.mem_block.syscall_cycles.clone(),
    );

    // check sender's balance
    // NOTE: for tx from id zero, its balance will be verified after mock account
    let from_id: u32 = raw_l2tx.from_id().unpack();
    if 0 != from_id {
        let check_balance_result = match block_number_opt {
            Some(block_number) => {
                let state = db.state_tree(StateContext::ReadOnlyHistory(block_number))?;
                verify_sender_balance(ctx.generator.rollup_context(), &state, &raw_l2tx)
            }
            None => {
                let snap = ctx.mem_pool_state.load();
                let state = snap.state()?;
                verify_sender_balance(ctx.generator.rollup_context(), &state, &raw_l2tx)
            }
        };
        if let Err(err) = check_balance_result {
            return Err(RpcError::Full {
                code: INVALID_REQUEST,
                message: format!("check balance err: {}", err),
                data: None,
            });
        }
    }

    // execute tx in task
    let metrics = ctx.metrics.clone();
    let mut run_result = tokio::task::spawn_blocking(move || {
        let eth_recover = &ctx.polyjuice_sender_recover.eth;
        let rollup_context = ctx.generator.rollup_context();
        let chain_view = {
            let tip_block_hash = db.get_last_valid_tip_block_hash()?;
            ChainView::new(&db, tip_block_hash)
        };
        // execute tx
        let run_result = match block_number_opt {
            Some(block_number) => {
                let hist_state = db.state_tree(StateContext::ReadOnlyHistory(block_number))?;
                let mut state = MemExecuteTxStateTree::new(hist_state);
                let raw_l2tx = eth_recover.mock_sender_if_not_exists_from_raw_registry(
                    raw_l2tx,
                    registry_address_opt,
                    &mut state,
                )?;
                if 0 == from_id {
                    verify_sender_balance(rollup_context, &state, &raw_l2tx)
                        .map_err(|err| anyhow!("check balance err {}", err))?;
                }

                ctx.generator.unchecked_execute_transaction(
                    &chain_view,
                    &state,
                    &block_info,
                    &raw_l2tx,
                    execute_l2tx_max_cycles,
                    Some(&mut cycles_pool),
                )?
            }
            None => {
                let state = mem_state_snap.state()?;
                let mut state = MemExecuteTxStateTree::new(state);
                let raw_l2tx = eth_recover.mock_sender_if_not_exists_from_raw_registry(
                    raw_l2tx,
                    registry_address_opt,
                    &mut state,
                )?;
                if 0 == from_id {
                    verify_sender_balance(rollup_context, &state, &raw_l2tx)
                        .map_err(|err| anyhow!("check balance err {}", err))?;
                }

                ctx.generator.unchecked_execute_transaction(
                    &chain_view,
                    &state,
                    &block_info,
                    &raw_l2tx,
                    execute_l2tx_max_cycles,
                    Some(&mut cycles_pool),
                )?
            }
        };
        Result::<_, anyhow::Error>::Ok(run_result)
    })
    .await??;

    metrics
        .get_or_create(&RunResultLabel {
            exit_code: run_result.exit_code,
        })
        .inc();

    if run_result.exit_code != 0 {
        let receipt = gw_types::offchain::ErrorTxReceipt {
            tx_hash,
            block_number,
            return_data: run_result.return_data,
            last_log: run_result.write.logs.pop(),
            exit_code: run_result.exit_code,
        };

        return Err(RpcError::Full {
            code: INVALID_REQUEST,
            message: TransactionError::InvalidExitCode(run_result.exit_code).to_string(),
            data: Some(Box::new(ErrorTxReceipt::from(receipt))),
        });
    }

    Ok(run_result.into())
}

#[allow(clippy::type_complexity)]
#[instrument(skip_all)]
async fn submit_l2transaction(
    Params((l2tx,)): Params<(JsonBytes,)>,
    ctx: Data<SubmitTransactionContext>,
) -> Result<Option<JsonH256>, RpcError> {
    let l2tx_bytes = l2tx.into_bytes();
    let tx = packed::L2Transaction::from_slice(&l2tx_bytes)?;
    let tx_hash: H256 = tx.hash().into();

    let sender_id: u32 = tx.raw().from_id().unpack();
    let eth_recover = &ctx.polyjuice_sender_recover.eth;
    if 0 == sender_id && eth_recover.opt_account_creator.is_none() {
        return Err(RpcError::Provided {
            code: METHOD_NOT_AVAILABLE_ERR_CODE,
            message: "tx from zero is disabled",
        });
    }

    // Return None for tx from zero because its from id will be updated after account creation.
    let tx_hash_json = if 0 == sender_id {
        None
    } else {
        Some(to_jsonh256(tx.hash().into()))
    };

    // check rate limit
    if let Some(rate_limiter) = ctx.rate_limiter.as_ref() {
        let mut rate_limiter = rate_limiter.lock().await;
        let sender_id: u32 = tx.raw().from_id().unpack();
        if let Some(last_touch) = rate_limiter.get(&sender_id) {
            if last_touch.elapsed().as_secs()
                < ctx
                    .rate_limit_config
                    .as_ref()
                    .map(|c| c.seconds)
                    .unwrap_or_default()
            {
                return Err(rate_limit_err());
            }
        }
        rate_limiter.put(sender_id, Instant::now());
    }

    // check sender's nonce
    {
        // fetch mem-pool state
        let snap = ctx.mem_pool_state.load();
        let tree = snap.state()?;

        let tx_nonce: u32 = tx.raw().nonce().unpack();
        let sender_nonce: u32 = if 0 == sender_id {
            0
        } else {
            tree.get_nonce(sender_id)?
        };
        if sender_nonce != tx_nonce {
            let err = TransactionError::Nonce {
                account_id: sender_id,
                expected: sender_nonce,
                actual: tx_nonce,
            };
            log::info!(
                "[RPC] reject to submit tx {:?}, err: {}",
                faster_hex::hex_string(&tx.hash()),
                err
            );
            return Err(RpcError::Full {
                code: INVALID_NONCE_ERR_CODE,
                message: err.to_string(),
                data: None,
            });
        }
    }

    let permit = ctx.submit_tx.try_reserve().map_err(|err| match err {
        mpsc::error::TrySendError::Closed(_) => RpcError::Provided {
            code: INTERNAL_ERROR_ERR_CODE,
            message: "internal error, unavailable",
        },
        mpsc::error::TrySendError::Full(_) => RpcError::Provided {
            code: BUSY_ERR_CODE,
            message: "mem pool service busy",
        },
    })?;

    let tx_hash_in_queue = match tx_hash_json {
        Some(_) => tx_hash,
        None => {
            let mut hasher = new_blake2b();
            let sig: Bytes = tx.signature().unpack();
            hasher.update(&sig);
            let mut hash = [0u8; 32];
            hasher.finalize(&mut hash);
            H256::from(hash)
        }
    };
    let request = Request::Tx(tx);
    // Use permit to insert before send so that remove won't happen before insert.
    if let Some(handle) = ctx
        .in_queue_request_map
        .as_ref()
        .expect("in_queue_request_map")
        .insert(tx_hash_in_queue, request.clone())
    {
        // Send if the request wasn't already in the map.
        permit.send((handle, request));
    }

    Ok(tx_hash_json)
}

// TODO: refactor complex type.
// Either `RPCContext` or derive?
#[allow(clippy::type_complexity)]
#[instrument(skip_all)]
async fn submit_withdrawal_request(
    Params((withdrawal_request,)): Params<(JsonBytes,)>,
    generator: Data<Generator>,
    store: Data<Store>,
    in_queue_request_map: Data<Option<Arc<InQueueRequestMap>>>,
    submit_tx: Data<mpsc::Sender<(InQueueRequestHandle, Request)>>,
) -> Result<JsonH256, RpcError> {
    let withdrawal_bytes = withdrawal_request.into_bytes();
    let withdrawal = packed::WithdrawalRequestExtra::from_slice(&withdrawal_bytes)?;
    let withdrawal_hash = withdrawal.hash();

    let last_valid = store.get_last_valid_tip_block_hash()?;
    let last_valid = store
        .get_block_number(&last_valid)?
        .expect("tip block number");
    let finalized_custodians = store
        .get_block_post_finalized_custodian_capacity(last_valid)
        .expect("finalized custodians");
    let withdrawal_generator = gw_mem_pool::withdrawal::Generator::new(
        generator.rollup_context(),
        finalized_custodians.as_reader().unpack(),
    );
    if let Err(err) = withdrawal_generator.verify_remained_amount(&withdrawal.request()) {
        return Err(RpcError::Full {
            code: CUSTODIAN_NOT_ENOUGH_CODE,
            message: format!(
                "Withdrawal fund are still finalizing, please try again later. error: {}",
                err
            ),
            data: None,
        });
    }
    if let Err(err) = withdrawal_generator.verified_output(&withdrawal, &Default::default()) {
        return Err(RpcError::Full {
            code: INVALID_REQUEST,
            message: err.to_string(),
            data: None,
        });
    }

    let permit = submit_tx.try_reserve().map_err(|err| match err {
        mpsc::error::TrySendError::Closed(_) => RpcError::Provided {
            code: INTERNAL_ERROR_ERR_CODE,
            message: "internal error, unavailable",
        },
        mpsc::error::TrySendError::Full(_) => RpcError::Provided {
            code: BUSY_ERR_CODE,
            message: "mem pool service busy",
        },
    })?;

    let request = Request::Withdrawal(withdrawal);
    // Use permit to insert before send so that remove won't happen before insert.
    if let Some(handle) = in_queue_request_map
        .as_ref()
        .expect("in_queue_request_map")
        .insert(withdrawal_hash.into(), request.clone())
    {
        // Send if the request wasn't already in the map.
        permit.send((handle, request));
    }

    Ok(withdrawal_hash.into())
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
enum GetWithdrawalParams {
    Default((JsonH256,)),
    WithVerbose((JsonH256, u8)),
}

enum GetWithdrawalVerbose {
    WithdrawalWithStatus = 0,
    OnlyStatus = 1,
}

impl TryFrom<u8> for GetWithdrawalVerbose {
    type Error = u8;
    fn try_from(n: u8) -> Result<Self, u8> {
        let verbose = match n {
            0 => Self::WithdrawalWithStatus,
            1 => Self::OnlyStatus,
            _ => {
                return Err(n);
            }
        };
        Ok(verbose)
    }
}

async fn get_withdrawal(
    Params(param): Params<GetWithdrawalParams>,
    store: Data<Store>,
    rpc_client: Data<RPCClient>,
    in_queue_request_map: Data<Option<Arc<InQueueRequestMap>>>,
) -> Result<Option<WithdrawalWithStatus>, RpcError> {
    let (withdrawal_hash, verbose) = match param {
        GetWithdrawalParams::Default((withdrawal_hash,)) => (
            to_h256(withdrawal_hash),
            GetWithdrawalVerbose::WithdrawalWithStatus,
        ),
        GetWithdrawalParams::WithVerbose((withdrawal_hash, verbose)) => {
            let verbose = verbose
                .try_into()
                .map_err(|_err| invalid_param_err("invalid verbose param"))?;
            (to_h256(withdrawal_hash), verbose)
        }
    };

    if let Some(w) = in_queue_request_map
        .as_deref()
        .and_then(|m| m.get_withdrawal(&withdrawal_hash))
    {
        return Ok(Some(WithdrawalWithStatus {
            withdrawal: matches!(verbose, GetWithdrawalVerbose::WithdrawalWithStatus)
                .then(|| w.into()),
            status: WithdrawalStatus::Pending,
            ..Default::default()
        }));
    }
    let db = store.get_snapshot();
    if let Some(withdrawal) = db.get_mem_pool_withdrawal(&withdrawal_hash)? {
        let withdrawal_opt = match verbose {
            GetWithdrawalVerbose::OnlyStatus => None,
            GetWithdrawalVerbose::WithdrawalWithStatus => Some(withdrawal.into()),
        };
        return Ok(Some(WithdrawalWithStatus {
            status: WithdrawalStatus::Pending,
            withdrawal: withdrawal_opt,
            ..Default::default()
        }));
    }
    if let Some(withdrawal_info) = db.get_withdrawal_info(&withdrawal_hash)? {
        if let Some(withdrawal) = db.get_withdrawal_by_key(&withdrawal_info.key())? {
            let withdrawal_opt = match verbose {
                GetWithdrawalVerbose::OnlyStatus => None,
                GetWithdrawalVerbose::WithdrawalWithStatus => Some(withdrawal.into()),
            };
            let l2_block_number: u64 = withdrawal_info.block_number().unpack();
            let l2_block_hash =
                packed::Byte32::from_slice(&withdrawal_info.key().as_slice()[..32])?.unpack();
            let l2_withdrawal_index: u32 =
                packed::Uint32::from_slice(&withdrawal_info.key().as_slice()[32..36])?.unpack();
            let l2_committed_info = Some(L2WithdrawalCommittedInfo {
                block_number: l2_block_number.into(),
                block_hash: to_jsonh256(l2_block_hash),
                withdrawal_index: l2_withdrawal_index.into(),
            });
            let l1_committed_info =
                get_block_committed_info(Params((to_jsonh256(l2_block_hash),)), rpc_client, store)
                    .await?;
            return Ok(Some(WithdrawalWithStatus {
                status: WithdrawalStatus::Committed,
                withdrawal: withdrawal_opt,
                l2_committed_info,
                l1_committed_info,
            }));
        }
    }
    Ok(None)
}

// registry address, sudt_id, block_number
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
enum GetBalanceParams {
    Tip((JsonBytes, AccountID)),
    Number((JsonBytes, AccountID, Option<GwUint64>)),
}

async fn get_balance(
    Params(params): Params<GetBalanceParams>,
    store: Data<Store>,
    mem_pool_state: Data<Arc<MemPoolState>>,
) -> Result<U256, RpcError> {
    let (serialized_address, sudt_id, block_number) = match params {
        GetBalanceParams::Tip(p) => (p.0, p.1, None),
        GetBalanceParams::Number(p) => p,
    };

    let address =
        gw_common::registry_address::RegistryAddress::from_slice(serialized_address.as_bytes())
            .ok_or_else(|| invalid_param_err("Invalid registry address"))?;
    let balance = match block_number {
        Some(block_number) => {
            let db = store.begin_transaction();
            let tree = db.state_tree(StateContext::ReadOnlyHistory(block_number.into()))?;
            tree.get_sudt_balance(sudt_id.into(), &address)?
        }
        None => {
            let snap = mem_pool_state.load();
            let tree = snap.state()?;
            tree.get_sudt_balance(sudt_id.into(), &address)?
        }
    };
    Ok(balance)
}

// account_id, key, block_number
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
enum GetStorageAtParams {
    Tip((AccountID, JsonH256)),
    Number((AccountID, JsonH256, Option<GwUint64>)),
}

async fn get_storage_at(
    Params(params): Params<GetStorageAtParams>,
    store: Data<Store>,
    mem_pool_state: Data<Arc<MemPoolState>>,
) -> Result<JsonH256, RpcError> {
    let (account_id, key, block_number) = match params {
        GetStorageAtParams::Tip(p) => (p.0, p.1, None),
        GetStorageAtParams::Number(p) => p,
    };

    let value = match block_number {
        Some(block_number) => {
            let db = store.begin_transaction();
            let tree = db.state_tree(StateContext::ReadOnlyHistory(block_number.into()))?;
            let key: H256 = to_h256(key);
            tree.get_value(account_id.into(), key.as_slice())?
        }
        None => {
            let snap = mem_pool_state.load();
            let tree = snap.state()?;
            let key: H256 = to_h256(key);
            tree.get_value(account_id.into(), key.as_slice())?
        }
    };

    let json_value = to_jsonh256(value);
    Ok(json_value)
}

async fn get_account_id_by_script_hash(
    Params((script_hash,)): Params<(JsonH256,)>,
    mem_pool_state: Data<Arc<MemPoolState>>,
) -> Result<Option<AccountID>, RpcError> {
    let snap = mem_pool_state.load();
    let tree = snap.state()?;

    let script_hash = to_h256(script_hash);

    let account_id_opt = tree
        .get_account_id_by_script_hash(&script_hash)?
        .map(Into::into);

    Ok(account_id_opt)
}

// account_id, block_number
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
enum GetNonceParams {
    Tip((AccountID,)),
    Number((AccountID, Option<GwUint64>)),
}

async fn get_nonce(
    Params(params): Params<GetNonceParams>,
    store: Data<Store>,
    mem_pool_state: Data<Arc<MemPoolState>>,
) -> Result<Uint32, RpcError> {
    let (account_id, block_number) = match params {
        GetNonceParams::Tip(p) => (p.0, None),
        GetNonceParams::Number(p) => p,
    };

    let nonce = match block_number {
        Some(block_number) => {
            let db = store.begin_transaction();
            let tree = db.state_tree(StateContext::ReadOnlyHistory(block_number.into()))?;
            tree.get_nonce(account_id.into())?
        }
        None => {
            let snap = mem_pool_state.load();
            let tree = snap.state()?;
            tree.get_nonce(account_id.into())?
        }
    };

    Ok(nonce.into())
}

async fn get_script(
    Params((script_hash,)): Params<(JsonH256,)>,
    mem_pool_state: Data<Arc<MemPoolState>>,
) -> Result<Option<Script>, RpcError> {
    let snap = mem_pool_state.load();
    let tree = snap.state()?;

    let script_hash = to_h256(script_hash);
    let script_opt = tree.get_script(&script_hash).map(Into::into);

    Ok(script_opt)
}

async fn get_script_hash(
    Params((account_id,)): Params<(AccountID,)>,
    mem_pool_state: Data<Arc<MemPoolState>>,
) -> Result<JsonH256, RpcError> {
    let snap = mem_pool_state.load();
    let tree = snap.state()?;

    let script_hash = tree.get_script_hash(account_id.into())?;
    Ok(to_jsonh256(script_hash))
}

async fn get_script_hash_by_registry_address(
    Params((serialized_address,)): Params<(JsonBytes,)>,
    mem_pool_state: Data<Arc<MemPoolState>>,
) -> Result<Option<JsonH256>, RpcError> {
    let snap = mem_pool_state.load();
    let tree = snap.state()?;
    let addr =
        gw_common::registry_address::RegistryAddress::from_slice(serialized_address.as_bytes())
            .ok_or_else(|| invalid_param_err("Invalid registry address"))?;
    let script_hash_opt = tree.get_script_hash_by_registry_address(&addr)?;
    Ok(script_hash_opt.map(to_jsonh256))
}

async fn get_registry_address_by_script_hash(
    Params((script_hash, registry_id)): Params<(JsonH256, Uint32)>,
    mem_pool_state: Data<Arc<MemPoolState>>,
) -> Result<Option<RegistryAddress>, RpcError> {
    let snap = mem_pool_state.load();
    let tree = snap.state()?;
    let addr =
        tree.get_registry_address_by_script_hash(registry_id.value(), &to_h256(script_hash))?;
    Ok(addr.map(Into::into))
}

// data_hash, block_number
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
enum GetDataParams {
    Tip((JsonH256,)),
    Number((JsonH256, Option<GwUint64>)),
}

async fn get_data(
    Params(params): Params<GetDataParams>,
    mem_pool_state: Data<Arc<MemPoolState>>,
) -> Result<Option<JsonBytes>, RpcError> {
    let (data_hash, _block_number) = match params {
        GetDataParams::Tip(p) => (p.0, None),
        GetDataParams::Number(p) => p,
    };

    let snap = mem_pool_state.load();
    let tree = snap.state()?;

    let data_opt = tree
        .get_data(&to_h256(data_hash))
        .map(JsonBytes::from_bytes);

    Ok(data_opt)
}

async fn compute_l2_sudt_script_hash(
    Params((l1_sudt_script_hash,)): Params<(JsonH256,)>,
    generator: Data<Generator>,
) -> Result<JsonH256> {
    let l2_sudt_script =
        build_l2_sudt_script(generator.rollup_context(), &to_h256(l1_sudt_script_hash));
    Ok(to_jsonh256(l2_sudt_script.hash().into()))
}

fn get_backend_info(generator: Arc<Generator>) -> Vec<BackendInfo> {
    generator
        .backend_manage()
        .get_backends_at_height(0)
        .expect("backends")
        .1
        .values()
        .map(|b| BackendInfo {
            validator_code_hash: ckb_fixed_hash::H256(b.checksum.validator.into()),
            generator_code_hash: ckb_fixed_hash::H256(b.checksum.generator.into()),
            validator_script_type_hash: ckb_fixed_hash::H256(b.validator_script_type_hash.into()),
            backend_type: to_rpc_backend_type(&b.backend_type),
        })
        .collect()
}

fn to_rpc_backend_type(b_type: &gw_config::BackendType) -> BackendType {
    match b_type {
        gw_config::BackendType::EthAddrReg => BackendType::EthAddrReg,
        gw_config::BackendType::Meta => BackendType::Meta,
        gw_config::BackendType::Sudt => BackendType::Sudt,
        gw_config::BackendType::Polyjuice => BackendType::Polyjuice,
        _ => BackendType::Unknown,
    }
}

pub fn to_node_rollup_config(rollup_config: &RollupConfig) -> NodeRollupConfig {
    let required_staking_capacity: GwUint64 = rollup_config
        .required_staking_capacity()
        .as_reader()
        .unpack()
        .into();
    let challenge_maturity_blocks: GwUint64 = rollup_config
        .challenge_maturity_blocks()
        .as_reader()
        .unpack()
        .into();
    let finality_blocks: GwUint64 = rollup_config.finality_blocks().as_reader().unpack().into();
    let burn_rate: u32 =
        bytes_v10::Buf::get_u8(&mut rollup_config.reward_burn_rate().as_bytes()).into();
    let reward_burn_rate: GwUint32 = burn_rate.into();
    let chain_id: GwUint64 = rollup_config.chain_id().as_reader().unpack().into();
    NodeRollupConfig {
        required_staking_capacity,
        challenge_maturity_blocks,
        finality_blocks,
        reward_burn_rate,
        chain_id,
    }
}

pub fn to_rollup_cell(chain_config: &ChainConfig) -> RollupCell {
    let type_hash: ckb_types::H256 = chain_config.rollup_type_script.hash();
    let type_script = chain_config.rollup_type_script.to_owned();
    RollupCell {
        type_hash,
        type_script,
    }
}

pub fn to_gw_scripts(
    rollup_config: &RollupConfig,
    consensus_config: &ConsensusConfig,
) -> Vec<GwScript> {
    let mut vec = Vec::new();

    let script = consensus_config
        .contract_type_scripts
        .state_validator
        .to_owned();
    let state_validator = GwScript {
        type_hash: script.hash(),
        script,
        script_type: GwScriptType::StateValidator,
    };
    vec.push(state_validator);

    let type_hash: ckb_types::H256 = rollup_config.deposit_script_type_hash().unpack();
    let script = consensus_config
        .contract_type_scripts
        .deposit_lock
        .to_owned();
    let deposit = GwScript {
        type_hash,
        script,
        script_type: GwScriptType::Deposit,
    };
    vec.push(deposit);

    let type_hash: ckb_types::H256 = rollup_config.withdrawal_script_type_hash().unpack();
    let script = consensus_config
        .contract_type_scripts
        .withdrawal_lock
        .to_owned();
    let withdraw = GwScript {
        type_hash,
        script,
        script_type: GwScriptType::Withdraw,
    };
    vec.push(withdraw);

    let type_hash: ckb_types::H256 = rollup_config.stake_script_type_hash().unpack();
    let script = consensus_config.contract_type_scripts.stake_lock.to_owned();
    let stake_lock = GwScript {
        type_hash,
        script,
        script_type: GwScriptType::StakeLock,
    };
    vec.push(stake_lock);

    let type_hash: ckb_types::H256 = rollup_config.custodian_script_type_hash().unpack();
    let script = consensus_config
        .contract_type_scripts
        .custodian_lock
        .to_owned();
    let custodian = GwScript {
        type_hash,
        script,
        script_type: GwScriptType::CustodianLock,
    };
    vec.push(custodian);

    let type_hash: ckb_types::H256 = rollup_config.withdrawal_script_type_hash().unpack();
    let script = consensus_config
        .contract_type_scripts
        .challenge_lock
        .to_owned();
    let challenge = GwScript {
        type_hash,
        script,
        script_type: GwScriptType::ChallengeLock,
    };
    vec.push(challenge);

    let type_hash: ckb_types::H256 = rollup_config.l1_sudt_script_type_hash().unpack();
    let script = consensus_config.contract_type_scripts.l1_sudt.to_owned();
    let l1_sudt = GwScript {
        type_hash,
        script,
        script_type: GwScriptType::L1Sudt,
    };
    vec.push(l1_sudt);

    let type_hash: ckb_types::H256 = rollup_config.l2_sudt_validator_script_type_hash().unpack();
    let script = consensus_config
        .contract_type_scripts
        .allowed_contract_scripts[&type_hash]
        .to_owned();
    let l2_sudt = GwScript {
        type_hash,
        script,
        script_type: GwScriptType::L2Sudt,
    };
    vec.push(l2_sudt);

    let script = consensus_config.contract_type_scripts.omni_lock.to_owned();
    let type_hash: ckb_types::H256 = script.hash();
    let omni_lock = GwScript {
        type_hash,
        script,
        script_type: GwScriptType::OmniLock,
    };
    vec.push(omni_lock);

    vec
}

pub fn to_eoa_scripts(
    rollup_config: &RollupConfig,
    consensus_config: &ConsensusConfig,
) -> Vec<EoaScript> {
    let mut vec = Vec::new();

    let a_type_hash = rollup_config
        .allowed_eoa_type_hashes()
        .get(0)
        .expect("idx 0 not exits in allowed_eoa_type_hashes");
    let a_type_hash_value = a_type_hash.hash();

    let mut hash: [u8; 32] = [0; 32];
    let source = a_type_hash_value.as_slice();
    hash.copy_from_slice(source);
    let type_hash: JsonH256 = hash.into();

    let script = consensus_config.contract_type_scripts.allowed_eoa_scripts[&type_hash].to_owned();
    let eth_eoa_script = EoaScript {
        type_hash,
        script,
        eoa_type: EoaScriptType::Eth,
    };
    vec.push(eth_eoa_script);

    vec
}

pub fn to_rpc_node_mode(node_mode: &NodeMode) -> RpcNodeMode {
    match node_mode {
        NodeMode::FullNode => RpcNodeMode::FullNode,
        NodeMode::ReadOnly => RpcNodeMode::ReadOnly,
        NodeMode::Test => RpcNodeMode::Test,
    }
}

async fn get_node_info(
    node_mode: Data<NodeMode>,
    backend_info: Data<Vec<BackendInfo>>,
    rollup_config: Data<RollupConfig>,
    (consensus_config, chain_config): (Data<ConsensusConfig>, Data<ChainConfig>),
) -> Result<NodeInfo> {
    let mode = to_rpc_node_mode(&node_mode);
    let node_rollup_config = to_node_rollup_config(&rollup_config);
    let rollup_cell = to_rollup_cell(&chain_config);
    let gw_scripts = to_gw_scripts(&rollup_config, &consensus_config);
    let eoa_scripts = to_eoa_scripts(&rollup_config, &consensus_config);

    Ok(NodeInfo {
        mode,
        version: Version::current().to_string(),
        backends: backend_info.clone(),
        rollup_config: node_rollup_config,
        rollup_cell,
        gw_scripts,
        eoa_scripts,
    })
}

async fn get_last_submitted_info(store: Data<Store>) -> Result<LastL2BlockCommittedInfo> {
    let last_submitted = store
        .get_last_submitted_block_number_hash()
        .context("get last submitted block")?
        .number()
        .unpack();
    let tx_hash = store
        .get_block_submit_tx_hash(last_submitted)
        .context("get submission tx hash")?;
    Ok(LastL2BlockCommittedInfo {
        transaction_hash: to_jsonh256(tx_hash),
    })
}

async fn get_fee_config(
    config: Data<Arc<ArcSwap<DynamicConfigManager>>>,
) -> Result<gw_jsonrpc_types::godwoken::FeeConfig> {
    let config = config.load();
    let fee = config.get_fee_config();
    let fee_config = gw_jsonrpc_types::godwoken::FeeConfig {
        meta_cycles_limit: fee.meta_cycles_limit.into(),
        sudt_cycles_limit: fee.sudt_cycles_limit.into(),
        withdraw_cycles_limit: fee.withdraw_cycles_limit.into(),
    };
    Ok(fee_config)
}

async fn get_mem_pool_state_root(
    mem_pool_state: Data<Arc<MemPoolState>>,
) -> Result<JsonH256, RpcError> {
    let snap = mem_pool_state.load();
    let tree = snap.state()?;
    let root = tree.calculate_root()?;
    Ok(to_jsonh256(root))
}

async fn get_mem_pool_state_ready(
    mem_pool_state: Data<Arc<MemPoolState>>,
) -> Result<bool, RpcError> {
    Ok(mem_pool_state.completed_initial_syncing())
}

async fn tests_produce_block(
    Params((payload,)): Params<(TestModePayload,)>,
    tests_rpc_impl: Data<BoxedTestsRPCImpl>,
) -> Result<()> {
    tests_rpc_impl.produce_block(payload).await
}

async fn tests_get_global_state(tests_rpc_impl: Data<BoxedTestsRPCImpl>) -> Result<GlobalState> {
    tests_rpc_impl.get_global_state().await
}

async fn start_profiler() -> Result<()> {
    log::info!("profiler started");
    *PROFILER_GUARD.lock().await = Some(ProfilerGuard::new(100).unwrap());
    Ok(())
}

async fn report_pprof() -> Result<()> {
    if let Some(profiler) = PROFILER_GUARD.lock().await.take() {
        if let Ok(report) = profiler.report().build() {
            let file = std::fs::File::create("/code/workspace/flamegraph.svg").unwrap();
            let mut options = pprof::flamegraph::Options::default();
            options.image_width = Some(2500);
            report.flamegraph_with_options(file, &mut options).unwrap();

            // output profile.proto with protobuf feature enabled
            // > https://github.com/tikv/pprof-rs#use-with-pprof
            use pprof::protos::Message;
            let mut file = std::fs::File::create("/code/workspace/profile.pb").unwrap();
            let profile = report.pprof().unwrap();
            let mut content = Vec::new();
            profile.encode(&mut content).unwrap();
            std::io::Write::write_all(&mut file, &content).unwrap();
        }
    }
    Ok(())
}

async fn get_rocksdb_memory_stats(store: Data<Store>) -> Result<Vec<CfMemStat>, RpcError> {
    Ok(store.gather_mem_stats())
}

async fn dump_jemalloc_profiling() -> Result<()> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let filename = format!("godwoken-jeprof-{}-heap", timestamp);

    let mut filename0 = format!("{}\0", filename);
    let opt_name = "prof.dump";
    let opt_c_name = std::ffi::CString::new(opt_name).unwrap();
    log::info!("jemalloc profiling dump: {}", filename);
    unsafe {
        let ret = jemalloc_sys::mallctl(
            opt_c_name.as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut filename0 as *mut _ as *mut _,
            std::mem::size_of::<*mut std::ffi::c_void>(),
        );
        if ret != 0 {
            log::error!("dump failure {:?}", errno::Errno(ret));
        }
    }

    Ok(())
}

// Reload config dynamically and return the difference between two configs.
async fn reload_config(
    dynamic_config_manager: Data<Arc<ArcSwap<DynamicConfigManager>>>,
) -> Result<DynamicConfigReloadResponse> {
    gw_dynamic_config::reload(dynamic_config_manager.clone()).await
}
