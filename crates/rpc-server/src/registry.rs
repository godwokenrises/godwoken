use std::{
    convert::TryInto,
    fmt::Display,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use ckb_types::prelude::{Builder, Entity};
use gw_common::blake2b::new_blake2b;
use gw_common::builtins::{CKB_SUDT_ACCOUNT_ID, ETH_REGISTRY_ACCOUNT_ID};
use gw_common::state::State;
use gw_config::{
    BackendForkConfig, ChainConfig, ConsensusConfig, FeeConfig, GaslessTxSupportConfig,
    MemPoolConfig, NodeMode, RPCMethods, RPCRateLimit, RPCServerConfig, SyscallCyclesConfig,
};
use gw_dynamic_config::manager::{DynamicConfigManager, DynamicConfigReloadResponse};
use gw_generator::backend_manage::BackendManage;
use gw_generator::generator::CyclesPool;
use gw_generator::utils::get_tx_type;
use gw_generator::{
    error::TransactionError, sudt::build_l2_sudt_script,
    verification::transaction::TransactionVerifier, ArcSwap, Generator,
};
use gw_jsonrpc_types::{
    blockchain::Script,
    ckb_jsonrpc_types::{JsonBytes, Uint32, Uint64},
    debug::DebugRunResult,
    godwoken::*,
    test_mode::TestModePayload,
};
use gw_mem_pool::fee::{
    queue::FeeQueue,
    types::{FeeEntry, FeeItem, FeeItemKind, FeeItemSender},
};
use gw_polyjuice_sender_recover::recover::PolyjuiceSenderRecover;
use gw_rpc_client::rpc_client::RPCClient;
use gw_store::state::history::history_state::RWConfig;
use gw_store::state::{BlockStateDB, MemStateDB};
use gw_store::{
    chain_view::ChainView, mem_pool_state::MemPoolState, traits::chain_store::ChainStore,
    CfMemStat, Store,
};
use gw_telemetry::traits::{TelemetryContext, TelemetryContextNewSpan, TelemetrySpanExt};
use gw_traits::CodeStore;
use gw_types::packed::RawL2Transaction;
use gw_types::{
    bytes::Bytes,
    h256::*,
    packed::{self, BlockInfo, Byte32, L2Transaction, RollupConfig, WithdrawalRequestExtra},
    prelude::*,
    U256,
};
use gw_utils::RollupContext;
use gw_version::Version;
use jsonrpc_core::{ErrorCode, MetaIoHandler};
use jsonrpc_utils::{pub_sub::Session, rpc};
use lru::LruCache;
use once_cell::sync::Lazy;
use pprof::ProfilerGuard;
use tokio::sync::{mpsc, Mutex};
use tracing::instrument;

use crate::apis::debug::replay_transaction;
use crate::in_queue_request_map::{InQueueRequestHandle, InQueueRequestMap};
use crate::utils::{to_h256, to_jsonh256};

static PROFILER_GUARD: Lazy<tokio::sync::Mutex<Option<ProfilerGuard>>> =
    Lazy::new(|| tokio::sync::Mutex::new(None));

// type alias
type MemPool = Option<Arc<Mutex<gw_mem_pool::pool::MemPool>>>;
type AccountID = Uint32;
type JsonH256 = ckb_fixed_hash::H256;
pub type BoxedTestModeRpc = Arc<dyn TestModeRpc + Send + Sync + 'static>;
type RpcNodeMode = gw_jsonrpc_types::godwoken::NodeMode;

const HEADER_NOT_FOUND_ERR_CODE: i64 = -32000;
const INVALID_NONCE_ERR_CODE: i64 = -32001;
const BUSY_ERR_CODE: i64 = -32006;
const CUSTODIAN_NOT_ENOUGH_CODE: i64 = -32007;

type SendTransactionRateLimiter = Mutex<LruCache<u32, Instant>>;

/// Wrapper of jsonrpc_core::Error that implements From<E> where E: Display.
pub struct MyRpcError(pub jsonrpc_core::Error);

pub type Result<T, E = MyRpcError> = std::result::Result<T, E>;

impl From<MyRpcError> for jsonrpc_core::Error {
    fn from(e: MyRpcError) -> Self {
        e.0
    }
}

impl<E: Display> From<E> for MyRpcError {
    fn from(e: E) -> Self {
        rpc_error(ErrorCode::InternalError, e.to_string())
    }
}

fn rpc_error(code: impl Into<ErrorCode>, message: impl Into<String>) -> MyRpcError {
    MyRpcError(jsonrpc_core::Error {
        code: code.into(),
        message: message.into(),
        data: None,
    })
}

fn rpc_error_with_data(
    code: impl Into<ErrorCode>,
    message: impl Into<String>,
    data: impl serde::ser::Serialize,
) -> MyRpcError {
    MyRpcError(jsonrpc_core::Error {
        code: code.into(),
        message: message.into(),
        data: Some(match serde_json::to_value(&data) {
            Ok(v) => v,
            Err(e) => return e.into(),
        }),
    })
}

fn method_not_found() -> MyRpcError {
    MyRpcError(jsonrpc_core::Error::method_not_found())
}

fn header_not_found_err() -> MyRpcError {
    rpc_error(HEADER_NOT_FOUND_ERR_CODE, "header not found")
}

#[rpc]
#[async_trait]
pub trait TestModeRpc {
    async fn tests_get_global_state(&self) -> Result<GlobalState>;
    async fn tests_produce_block(&self, payload: TestModePayload) -> Result<()>;
}

#[async_trait]
impl<T: TestModeRpc + Send + Sync + ?Sized> TestModeRpc for Arc<T> {
    async fn tests_get_global_state(&self) -> Result<GlobalState> {
        T::tests_get_global_state(self).await
    }
    async fn tests_produce_block(&self, payload: TestModePayload) -> Result<()> {
        T::tests_produce_block(self, payload).await
    }
}

pub struct RequestContext {
    _in_queue_handle: InQueueRequestHandle,
    trace: gw_telemetry::Context,
    in_queue_span: tracing::Span,
}

impl TelemetryContext for RequestContext {
    fn telemetry_context(&self) -> Option<&gw_telemetry::Context> {
        Some(&self.trace)
    }
}

impl Drop for RequestContext {
    fn drop(&mut self) {
        let _drop = self.trace.new_span(tracing::info_span!("drop")).entered();
    }
}

pub struct RegistryArgs {
    pub store: Store,
    pub mem_pool: MemPool,
    pub generator: Arc<Generator>,
    pub tests_rpc_impl: Option<BoxedTestModeRpc>,
    pub rollup_config: RollupConfig,
    pub mem_pool_config: MemPoolConfig,
    pub node_mode: NodeMode,
    pub rpc_client: RPCClient,
    pub send_tx_rate_limit: Option<RPCRateLimit>,
    pub server_config: RPCServerConfig,
    pub chain_config: ChainConfig,
    pub consensus_config: ConsensusConfig,
    pub gasless_tx_support_config: Option<GaslessTxSupportConfig>,
    pub dynamic_config_manager: Arc<ArcSwap<DynamicConfigManager>>,
    pub polyjuice_sender_recover: PolyjuiceSenderRecover,
    pub debug_backend_forks: Option<Vec<BackendForkConfig>>,
}

pub struct Registry {
    pub(crate) generator: Arc<Generator>,
    pub(crate) mem_pool: MemPool,
    pub(crate) store: Store,
    pub(crate) tests_rpc_impl: Option<BoxedTestModeRpc>,
    pub(crate) rollup_config: RollupConfig,
    pub(crate) mem_pool_config: MemPoolConfig,
    pub(crate) backend_info: Vec<BackendInfo>,
    pub(crate) node_mode: NodeMode,
    pub(crate) submit_tx: mpsc::Sender<(Request, RequestContext)>,
    pub(crate) rpc_client: RPCClient,
    pub(crate) send_tx_rate_limit: Option<SendTransactionRateLimiter>,
    pub(crate) send_tx_rate_limit_config: Option<RPCRateLimit>,
    pub(crate) server_config: RPCServerConfig,
    pub(crate) chain_config: ChainConfig,
    pub(crate) consensus_config: ConsensusConfig,
    pub(crate) gasless_tx_support_config: Option<GaslessTxSupportConfig>,
    pub(crate) dynamic_config_manager: Arc<ArcSwap<DynamicConfigManager>>,
    pub(crate) mem_pool_state: Arc<MemPoolState>,
    pub(crate) in_queue_request_map: Option<Arc<InQueueRequestMap>>,
    pub(crate) polyjuice_sender_recover: Arc<PolyjuiceSenderRecover>,
    pub(crate) debug_generator: Arc<Generator>,
}

impl Registry {
    pub async fn create(args: RegistryArgs) -> anyhow::Result<Arc<Self>> {
        let RegistryArgs {
            generator,
            mem_pool,
            store,
            tests_rpc_impl,
            rollup_config,
            mem_pool_config,
            node_mode,
            rpc_client,
            send_tx_rate_limit: send_tx_rate_limit_config,
            server_config,
            chain_config,
            consensus_config,
            dynamic_config_manager,
            polyjuice_sender_recover,
            debug_backend_forks,
            gasless_tx_support_config,
        } = args;

        let backend_info = get_backend_info(generator.clone());

        let mem_pool_state = match mem_pool.as_ref() {
            Some(pool) => {
                let mem_pool = pool.lock().await;
                mem_pool.mem_pool_state()
            }
            None => Arc::new(MemPoolState::new(
                MemStateDB::from_store(store.get_snapshot()).expect("mem state DB"),
                true,
            )),
        };
        let in_queue_request_map = if matches!(node_mode, NodeMode::FullNode | NodeMode::Test) {
            Some(Arc::new(InQueueRequestMap::default()))
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
                gasless_tx_support_config: gasless_tx_support_config.clone(),
            };
            tokio::spawn(submitter.in_background());
        }

        let send_tx_rate_limit: Option<SendTransactionRateLimiter> = send_tx_rate_limit_config
            .as_ref()
            .map(|send_tx_rate_limit| Mutex::new(lru::LruCache::new(send_tx_rate_limit.lru_size)));

        let debug_generator = match debug_backend_forks {
            Some(config) => {
                let backend_manage = BackendManage::from_config(config)?;
                Arc::new(generator.clone_with_new_backends(backend_manage))
            }
            None => {
                log::warn!("Enable debug RPC without setting the 'debug_backend_switches' option. Fallback to non-debugging version backends, the debug log may not work");
                generator.clone()
            }
        };

        Ok(Self {
            mem_pool,
            store,
            generator,
            tests_rpc_impl,
            rollup_config,
            mem_pool_config,
            backend_info,
            node_mode,
            submit_tx,
            rpc_client,
            send_tx_rate_limit,
            send_tx_rate_limit_config,
            server_config,
            chain_config,
            consensus_config,
            gasless_tx_support_config,
            dynamic_config_manager,
            mem_pool_state,
            in_queue_request_map,
            polyjuice_sender_recover,
            debug_generator,
        }
        .into())
    }

    pub fn to_handler(self: Arc<Self>) -> MetaIoHandler<Option<Session>> {
        let mut handler = MetaIoHandler::with_compatibility(jsonrpc_core::Compatibility::V2);
        if let Some(ref tests_rpc_impl) = self.tests_rpc_impl {
            add_test_mode_rpc_methods(&mut handler, tests_rpc_impl.clone());
        }
        add_gw_rpc_methods(&mut handler, self);
        handler
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
    submit_rx: mpsc::Receiver<(Request, RequestContext)>,
    queue: FeeQueue<RequestContext>,
    dynamic_config_manager: Arc<ArcSwap<DynamicConfigManager>>,
    generator: Arc<Generator>,
    mem_pool_state: Arc<MemPoolState>,
    store: Store,
    polyjuice_sender_recover: Arc<PolyjuiceSenderRecover>,
    mem_pool_config: MemPoolConfig,
    gasless_tx_support_config: Option<GaslessTxSupportConfig>,
}

#[instrument(skip_all, fields(req_kind = req.kind()))]
fn req_to_entry(
    fee_config: &FeeConfig,
    gasless_tx_support_config: Option<&GaslessTxSupportConfig>,
    generator: Arc<Generator>,
    req: Request,
    state: &(impl State + CodeStore),
    order: usize,
) -> anyhow::Result<FeeEntry> {
    match req {
        Request::Tx(tx) => {
            let receiver: u32 = tx.raw().to_id().unpack();
            let script_hash = state.get_script_hash(receiver)?;
            let backend_type = generator
                .load_backend_and_block_consensus(0, state, &script_hash)
                .ok_or_else(|| anyhow!("can't find backend for receiver: {}", receiver))?
                .0
                .backend_type;
            FeeEntry::from_tx(
                tx,
                gasless_tx_support_config,
                fee_config,
                backend_type,
                order,
            )
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
            let mut mem_pool = self.mem_pool.lock().await;
            let db = &self.store.begin_transaction();

            log::info!(
                "reinject mem block txs {}",
                mem_pool.pending_restored_tx_hashes().len()
            );

            // Use unlimit to ensure all exists mem pool transactions are included
            let mut org_cycles_pool = mem_pool.cycles_pool().clone();
            *mem_pool.cycles_pool_mut() = CyclesPool::new(u64::MAX, SyscallCyclesConfig::default());

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
                let (req, mut ctx) = match self.submit_rx.recv().await {
                    Some(req) => req,
                    None => {
                        log::error!("rpc submit tx is closed");
                        return;
                    }
                };

                gw_telemetry::with_span_ref(&ctx.in_queue_span, |span| span.end());
                ctx.in_queue_span = ctx.trace.new_span(tracing::info_span!("fee_queue.add"));
                let _entered = ctx.in_queue_span.clone().entered();

                let state = self.mem_pool_state.load_state_db();

                let kind = req.kind();
                let hash = req.hash();
                let dynamic_config_manager = self.dynamic_config_manager.load();
                let fee_config = dynamic_config_manager.get_fee_config();
                match req_to_entry(
                    fee_config,
                    self.gasless_tx_support_config.as_ref(),
                    self.generator.clone(),
                    req,
                    &state,
                    queue.len(),
                ) {
                    Ok(entry) => {
                        if entry.cycles_limit > self.mem_pool_config.mem_block.max_cycles_limit {
                            log::info!(
                                "req kind {} hash {} exceeded mem block max cycles limit, drop it",
                                kind,
                                hash,
                            );
                        } else {
                            queue.add(entry, ctx);
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
            let state = self.mem_pool_state.load_state_db();
            while let Ok((req, mut ctx)) = self.submit_rx.try_recv() {
                gw_telemetry::with_span_ref(&ctx.in_queue_span, |span| span.end());
                ctx.in_queue_span = ctx.trace.new_span(tracing::info_span!("fee_queue.add"));
                let _entered = ctx.in_queue_span.clone().entered();

                let kind = req.kind();
                let hash = req.hash();
                let dynamic_config_manager = self.dynamic_config_manager.load();
                let fee_config = dynamic_config_manager.get_fee_config();
                match req_to_entry(
                    fee_config,
                    self.gasless_tx_support_config.as_ref(),
                    self.generator.clone(),
                    req,
                    &state,
                    queue.len(),
                ) {
                    Ok(entry) => {
                        if entry.cycles_limit > self.mem_pool_config.mem_block.max_cycles_limit {
                            log::info!(
                                "req kind {} hash {} exceeded mem block max cycles limit, drop it",
                                kind,
                                hash,
                            );
                        } else {
                            queue.add(entry, ctx);
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

                let state = self.mem_pool_state.load_state_db();
                let mut block_cycles_limit_reached = false;

                for (entry, ctx) in items {
                    gw_telemetry::with_span_ref(&ctx.in_queue_span, |span| span.end());
                    let push_span = ctx.new_span(|_| tracing::info_span!("mem_pool.push"));
                    let _entered = push_span.enter();

                    if let FeeItemKind::Tx = entry.item.kind() {
                        if !block_cycles_limit_reached
                            && entry.cycles_limit > mem_pool.cycles_pool().available_cycles()
                        {
                            let hash: Byte32 = entry.item.hash().pack();
                            log::info!("mem block cycles limit reached for tx {}", hash);

                            block_cycles_limit_reached = true;
                        }

                        if block_cycles_limit_reached {
                            queue.add(entry, ctx);
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
                            queue.add(entry, ctx);

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

#[rpc]
#[async_trait]
pub trait GwRpc {
    async fn gw_ping(&self) -> Result<String>;
    async fn gw_get_transaction(
        &self,
        tx_hash: JsonH256,
        verbose: Option<GetVerbose>,
    ) -> Result<Option<L2TransactionWithStatus>>;
    async fn gw_get_pending_tx_hashes(&self) -> Result<Vec<JsonH256>>;
    async fn gw_is_request_in_queue(&self, hash: JsonH256) -> Result<bool>;
    async fn gw_get_block_committed_info(
        &self,
        block_hash: JsonH256,
    ) -> Result<Option<L2BlockCommittedInfo>>;
    async fn gw_get_block(&self, block_hash: JsonH256) -> Result<Option<L2BlockWithStatus>>;
    async fn gw_get_block_by_number(&self, block_number: Uint64) -> Result<Option<L2BlockView>>;
    async fn gw_get_block_hash(&self, block_number: Uint64) -> Result<Option<JsonH256>>;
    async fn gw_get_tip_block_hash(&self) -> Result<JsonH256>;
    async fn gw_get_transaction_receipt(&self, tx_hash: JsonH256) -> Result<Option<TxReceipt>>;
    async fn gw_execute_l2transaction(&self, l2tx: L2TransactionJsonBytes) -> Result<RunResult>;
    async fn gw_execute_raw_l2transaction(
        &self,
        tx: RawL2TransactionJsonBytes,
        block_number: Option<Uint64>,
        registry_address: Option<RegistryAddressJsonBytes>,
    ) -> Result<RunResult>;
    async fn gw_submit_l2transaction(
        &self,
        l2tx: L2TransactionJsonBytes,
    ) -> Result<Option<JsonH256>>;
    async fn gw_submit_withdrawal_request(
        &self,
        withdrawal_request: WithdrawalRequestExtraJsonBytes,
    ) -> Result<JsonH256>;
    async fn gw_get_withdrawal(
        &self,
        hash: JsonH256,
        verbose: Option<GetVerbose>,
    ) -> Result<Option<WithdrawalWithStatus>>;
    async fn gw_get_balance(
        &self,
        address: RegistryAddressJsonBytes,
        sudt_id: AccountID,
        block_number: Option<Uint64>,
    ) -> Result<U256>;
    async fn gw_get_storage_at(
        &self,
        account_id: AccountID,
        key: JsonH256,
        block_number: Option<Uint64>,
    ) -> Result<JsonH256>;
    async fn gw_get_account_id_by_script_hash(
        &self,
        script_hash: JsonH256,
    ) -> Result<Option<AccountID>>;
    async fn gw_get_nonce(
        &self,
        account_id: AccountID,
        block_number: Option<Uint64>,
    ) -> Result<Uint32>;
    async fn gw_get_script(&self, script_hash: JsonH256) -> Result<Option<Script>>;
    async fn gw_get_script_hash(&self, account_id: AccountID) -> Result<JsonH256>;
    async fn gw_get_script_hash_by_registry_address(
        &self,
        address: RegistryAddressJsonBytes,
    ) -> Result<Option<JsonH256>>;
    async fn gw_get_registry_address_by_script_hash(
        &self,
        script_hash: JsonH256,
        registry_id: Uint32,
    ) -> Result<Option<RegistryAddress>>;
    async fn gw_get_data(
        &self,
        data_hash: JsonH256,
        block_number: Option<Uint64>,
    ) -> Result<Option<JsonBytes>>;
    async fn gw_compute_l2_sudt_script_hash(
        &self,
        l1_sudt_script_hash: JsonH256,
    ) -> Result<JsonH256>;
    async fn gw_get_node_info(&self) -> Result<NodeInfo>;
    async fn gw_get_last_submitted_info(&self) -> Result<LastL2BlockCommittedInfo>;
    async fn gw_get_fee_config(&self) -> Result<gw_jsonrpc_types::godwoken::FeeConfig>;
    async fn gw_get_mem_pool_state_root(&self) -> Result<JsonH256>;
    async fn gw_get_mem_pool_state_ready(&self) -> Result<bool>;
    async fn gw_reload_config(&self) -> Result<DynamicConfigReloadResponse>;

    async fn gw_start_profiler(&self) -> Result<()>;
    async fn gw_report_pprof(&self) -> Result<()>;

    async fn gw_get_rocksdb_memory_stats(&self) -> Result<Vec<CfMemStat>>;
    async fn gw_dump_jemalloc_profiling(&self) -> Result<()>;

    async fn gw_replay_transaction(
        &self,
        tx_hash: JsonH256,
        max_cycles: Option<Uint64>,
    ) -> Result<Option<DebugRunResult>>;
}

#[async_trait]
impl GwRpc for Arc<Registry> {
    async fn gw_ping(&self) -> Result<String> {
        Ok("pong".into())
    }
    async fn gw_get_transaction(
        &self,
        tx_hash: JsonH256,
        verbose: Option<GetVerbose>,
    ) -> Result<Option<L2TransactionWithStatus>> {
        gw_get_transaction(self, tx_hash, verbose).await
    }
    #[instrument(skip_all)]
    async fn gw_get_pending_tx_hashes(&self) -> Result<Vec<JsonH256>> {
        let snap = self.store.get_snapshot();
        let tx_hashes = snap
            .iter_mem_pool_transactions()
            .map(|hash| JsonH256::from_slice(&hash).expect("transaction hash"))
            .collect();
        Ok(tx_hashes)
    }
    #[instrument(skip_all)]
    async fn gw_is_request_in_queue(&self, hash: JsonH256) -> Result<bool> {
        let hash = to_h256(hash);

        Ok(self
            .in_queue_request_map
            .as_deref()
            .map_or(false, |m| m.contains(&hash)))
    }
    async fn gw_get_block_committed_info(
        &self,
        block_hash: JsonH256,
    ) -> Result<Option<L2BlockCommittedInfo>> {
        gw_get_block_committed_info(block_hash, self).await
    }
    async fn gw_get_block(&self, block_hash: JsonH256) -> Result<Option<L2BlockWithStatus>> {
        gw_get_block(block_hash, &self.store, &self.rollup_config).await
    }
    async fn gw_get_block_by_number(&self, block_number: Uint64) -> Result<Option<L2BlockView>> {
        gw_get_block_by_number(self, block_number).await
    }
    async fn gw_get_block_hash(&self, block_number: Uint64) -> Result<Option<JsonH256>> {
        gw_get_block_hash(self, block_number).await
    }
    async fn gw_get_tip_block_hash(&self) -> Result<JsonH256> {
        gw_get_tip_block_hash(self).await
    }
    async fn gw_get_transaction_receipt(&self, tx_hash: JsonH256) -> Result<Option<TxReceipt>> {
        gw_get_transaction_receipt(self, tx_hash).await
    }
    async fn gw_execute_l2transaction(&self, l2tx: L2TransactionJsonBytes) -> Result<RunResult> {
        gw_execute_l2transaction(self.clone(), l2tx).await
    }
    async fn gw_execute_raw_l2transaction(
        &self,
        tx: RawL2TransactionJsonBytes,
        block_number: Option<Uint64>,
        registry_address: Option<RegistryAddressJsonBytes>,
    ) -> Result<RunResult> {
        gw_execute_raw_l2transaction(self.clone(), tx, block_number, registry_address).await
    }
    async fn gw_submit_l2transaction(
        &self,
        l2tx: L2TransactionJsonBytes,
    ) -> Result<Option<JsonH256>> {
        if self.node_mode == NodeMode::ReadOnly {
            return Err(method_not_found());
        }
        gw_submit_l2transaction(self, l2tx).await
    }
    async fn gw_submit_withdrawal_request(
        &self,
        withdrawal_request: WithdrawalRequestExtraJsonBytes,
    ) -> Result<JsonH256> {
        if self.node_mode == NodeMode::ReadOnly {
            return Err(method_not_found());
        }
        gw_submit_withdrawal_request(self, withdrawal_request).await
    }
    async fn gw_get_withdrawal(
        &self,
        hash: JsonH256,
        verbose: Option<GetVerbose>,
    ) -> Result<Option<WithdrawalWithStatus>> {
        gw_get_withdrawal(self, hash, verbose).await
    }
    async fn gw_get_balance(
        &self,
        address: RegistryAddressJsonBytes,
        sudt_id: AccountID,
        block_number: Option<Uint64>,
    ) -> Result<U256> {
        gw_get_balance(self, address, sudt_id, block_number).await
    }
    async fn gw_get_storage_at(
        &self,
        account_id: AccountID,
        key: JsonH256,
        block_number: Option<Uint64>,
    ) -> Result<JsonH256> {
        gw_get_storage_at(self, account_id, key, block_number).await
    }
    async fn gw_get_account_id_by_script_hash(
        &self,
        script_hash: JsonH256,
    ) -> Result<Option<AccountID>> {
        gw_get_account_id_by_script_hash(self, script_hash).await
    }
    async fn gw_get_nonce(
        &self,
        account_id: AccountID,
        block_number: Option<Uint64>,
    ) -> Result<Uint32> {
        gw_get_nonce(self, account_id, block_number).await
    }
    async fn gw_get_script(&self, script_hash: JsonH256) -> Result<Option<Script>> {
        gw_get_script(self, script_hash).await
    }
    async fn gw_get_script_hash(&self, account_id: AccountID) -> Result<JsonH256> {
        gw_get_script_hash(self, account_id).await
    }
    async fn gw_get_script_hash_by_registry_address(
        &self,
        address: RegistryAddressJsonBytes,
    ) -> Result<Option<JsonH256>> {
        gw_get_script_hash_by_registry_address(self, address).await
    }
    async fn gw_get_registry_address_by_script_hash(
        &self,
        script_hash: JsonH256,
        registry_id: Uint32,
    ) -> Result<Option<RegistryAddress>> {
        gw_get_registry_address_by_script_hash(self, script_hash, registry_id).await
    }
    #[instrument(skip_all)]
    async fn gw_get_data(
        &self,
        data_hash: JsonH256,
        _block_number: Option<Uint64>,
    ) -> Result<Option<JsonBytes>> {
        let state = self.mem_pool_state.load_state_db();
        let data_opt = state.get_data(&to_h256(data_hash));
        Ok(data_opt.map(JsonBytes::from_bytes))
    }
    #[instrument(skip_all)]
    async fn gw_compute_l2_sudt_script_hash(
        &self,
        l1_sudt_script_hash: JsonH256,
    ) -> Result<JsonH256> {
        let l2_sudt_script = build_l2_sudt_script(
            self.generator.rollup_context(),
            &to_h256(l1_sudt_script_hash),
        );
        Ok(to_jsonh256(l2_sudt_script.hash()))
    }
    #[instrument(skip_all)]
    async fn gw_get_node_info(&self) -> Result<NodeInfo> {
        let mode = to_rpc_node_mode(&self.node_mode);
        let node_rollup_config = to_node_rollup_config(&self.rollup_config);
        let rollup_cell = to_rollup_cell(&self.chain_config);
        let gw_scripts = to_gw_scripts(&self.rollup_config, &self.consensus_config);
        let eoa_scripts = to_eoa_scripts(&self.rollup_config, &self.consensus_config);

        Ok(NodeInfo {
            mode,
            version: Version::current().to_string(),
            backends: self.backend_info.clone(),
            rollup_config: node_rollup_config,
            rollup_cell,
            gw_scripts,
            eoa_scripts,
            gasless_tx_support: self.gasless_tx_support_config.clone(),
        })
    }
    #[instrument(skip_all)]
    async fn gw_get_last_submitted_info(&self) -> Result<LastL2BlockCommittedInfo> {
        let last_submitted = self
            .store
            .get_last_submitted_block_number_hash()
            .context("get last submitted block")?
            .number()
            .unpack();
        let tx_hash = self
            .store
            .get_block_submit_tx_hash(last_submitted)
            .context("get submission tx hash")?;
        Ok(LastL2BlockCommittedInfo {
            transaction_hash: to_jsonh256(tx_hash),
        })
    }
    #[instrument(skip_all)]
    async fn gw_get_fee_config(&self) -> Result<gw_jsonrpc_types::godwoken::FeeConfig> {
        let config = self.dynamic_config_manager.load();
        let fee = config.get_fee_config();
        let fee_config = gw_jsonrpc_types::godwoken::FeeConfig {
            meta_cycles_limit: fee.meta_cycles_limit.into(),
            sudt_cycles_limit: fee.sudt_cycles_limit.into(),
            withdraw_cycles_limit: fee.withdraw_cycles_limit.into(),
        };
        Ok(fee_config)
    }
    #[instrument(skip_all)]
    async fn gw_get_mem_pool_state_root(&self) -> Result<JsonH256> {
        let state = self.mem_pool_state.load_state_db();
        let root = state.last_state_root();
        Ok(to_jsonh256(root))
    }
    #[instrument(skip_all)]
    async fn gw_get_mem_pool_state_ready(&self) -> Result<bool> {
        Ok(self.mem_pool_state.completed_initial_syncing())
    }

    #[instrument(skip_all)]
    async fn gw_start_profiler(&self) -> Result<()> {
        if !self
            .server_config
            .enable_methods
            .contains(&RPCMethods::PProf)
        {
            return Err(method_not_found());
        }

        log::info!("profiler started");
        *PROFILER_GUARD.lock().await = Some(ProfilerGuard::new(100).unwrap());
        Ok(())
    }
    #[instrument(skip_all)]
    async fn gw_report_pprof(&self) -> Result<()> {
        if !self
            .server_config
            .enable_methods
            .contains(&RPCMethods::PProf)
        {
            return Err(method_not_found());
        }

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
    #[instrument(skip_all)]
    async fn gw_get_rocksdb_memory_stats(&self) -> Result<Vec<CfMemStat>> {
        if !self
            .server_config
            .enable_methods
            .contains(&RPCMethods::Test)
        {
            return Err(method_not_found());
        }

        Ok(self.store.gather_mem_stats())
    }
    #[instrument(skip_all)]
    async fn gw_dump_jemalloc_profiling(&self) -> Result<()> {
        if !self
            .server_config
            .enable_methods
            .contains(&RPCMethods::Test)
        {
            return Err(method_not_found());
        }

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
    #[instrument(skip_all)]
    async fn gw_reload_config(&self) -> Result<DynamicConfigReloadResponse> {
        Ok(gw_dynamic_config::reload(self.dynamic_config_manager.clone()).await?)
    }

    #[instrument(skip_all)]
    async fn gw_replay_transaction(
        &self,
        tx_hash: JsonH256,
        max_cycles: Option<Uint64>,
    ) -> Result<Option<DebugRunResult>> {
        if !self
            .server_config
            .enable_methods
            .contains(&RPCMethods::Debug)
        {
            return Err(method_not_found());
        }

        Ok(replay_transaction(self.clone(), tx_hash, max_cycles).await?)
    }
}

#[instrument(skip_all)]
async fn gw_get_transaction(
    ctx: &Registry,
    tx_hash: JsonH256,
    verbose: Option<GetVerbose>,
) -> Result<Option<L2TransactionWithStatus>> {
    let tx_hash = tx_hash.into();
    let verbose = verbose.unwrap_or_default();

    if let Some(tx) = ctx
        .in_queue_request_map
        .as_deref()
        .and_then(|m| m.get_transaction(&tx_hash))
    {
        return Ok(Some(L2TransactionWithStatus {
            transaction: verbose.verbose().then(|| tx.into()),
            status: L2TransactionStatus::Pending,
        }));
    }
    let db = ctx.store.get_snapshot();
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

    Ok(tx_opt.map(|tx| L2TransactionWithStatus {
        transaction: verbose.verbose().then(|| tx.into()),
        status,
    }))
}

#[instrument(skip_all)]
async fn gw_get_block_committed_info(
    block_hash: JsonH256,
    ctx: &Registry,
) -> Result<Option<L2BlockCommittedInfo>> {
    if let Some(number) = ctx.store.get_block_number(&to_h256(block_hash))? {
        if let Some(transaction_hash) = ctx.store.get_block_submit_tx_hash(number) {
            let opt_block_hash = ctx
                .rpc_client
                .ckb
                .get_transaction_block_hash(transaction_hash)
                .await?;
            if let Some(block_hash) = opt_block_hash {
                let number = ctx
                    .rpc_client
                    .get_header(block_hash)
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

#[instrument(skip_all)]
async fn gw_get_block(
    block_hash: JsonH256,
    store: &Store,
    rollup_config: &RollupConfig,
) -> Result<Option<L2BlockWithStatus>> {
    let block_hash = to_h256(block_hash);
    let mut db = store.begin_transaction();
    let block = match db.get_block(&block_hash)? {
        Some(block) => block,
        None => return Ok(None),
    };

    // check block status
    let mut status = L2BlockStatus::Unfinalized;
    if !db.reverted_block_smt()?.get(&block_hash.into())?.is_zero() {
        // block is reverted
        status = L2BlockStatus::Reverted;
    } else {
        // return None if block is not on the main chain
        if H256::from(db.block_smt()?.get(&block.smt_key().into())?) != block_hash {
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

// Why do we read from `MemPoolState` instead of `Store` for these “get block”
// RPCs:
//
// `MemPoolState` can fall behind `Store` (at the moment after a new block is
// inserted but mem pool state hasn't been updated). If we read from `Store`, we
// may get a block that is not in `MemPoolState` yet. If we then try to get
// scripts for accounts in the block, we may get an error response, because the
// `get_script` / `get_script_hash` RPCs use `MemPoolState`.
//
// Instead if we always read from `MemPoolState`, it is much less likely that we
// get an error response when getting scripts for accounts in the new block.
#[instrument(skip_all)]
async fn gw_get_block_by_number(
    ctx: &Registry,
    block_number: Uint64,
) -> Result<Option<L2BlockView>> {
    let block_number = block_number.value();
    let mem_store = ctx.mem_pool_state.load_mem_store();
    let block_hash = match mem_store.get_block_hash_by_number(block_number)? {
        Some(hash) => hash,
        None => return Ok(None),
    };
    let block_opt = mem_store.get_block(&block_hash)?.map(|block| {
        let block_view: L2BlockView = block.into();
        block_view
    });
    Ok(block_opt)
}

#[instrument(skip_all)]
async fn gw_get_block_hash(ctx: &Registry, block_number: Uint64) -> Result<Option<JsonH256>> {
    let block_number = block_number.value();
    let mem_store = ctx.mem_pool_state.load_mem_store();
    let hash_opt = mem_store
        .get_block_hash_by_number(block_number)?
        .map(to_jsonh256);
    Ok(hash_opt)
}

#[instrument(skip_all)]
async fn gw_get_tip_block_hash(ctx: &Registry) -> Result<JsonH256> {
    let mem_store = ctx.mem_pool_state.load_mem_store();
    let tip_block_hash = mem_store.get_last_valid_tip_block_hash()?;
    Ok(to_jsonh256(tip_block_hash))
}

#[instrument(skip_all)]
async fn gw_get_transaction_receipt(
    ctx: &Registry,
    tx_hash: JsonH256,
) -> Result<Option<TxReceipt>> {
    let tx_hash = to_h256(tx_hash);
    let db = ctx.store.get_snapshot();
    // search from db
    if let Some(receipt) = db.get_transaction_receipt(&tx_hash)? {
        return Ok(Some(receipt.into()));
    }
    // search from mem pool
    Ok(db
        .get_mem_pool_transaction_receipt(&tx_hash)?
        .map(Into::into))
}

#[instrument(skip_all, err(Debug))]
fn verify_sender_balance<S: State + CodeStore>(
    ctx: &RollupContext,
    state: &S,
    raw_tx: &RawL2Transaction,
) -> anyhow::Result<()> {
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
async fn gw_execute_l2transaction(
    ctx: Arc<Registry>,
    tx: L2TransactionJsonBytes,
) -> Result<RunResult> {
    if ctx.mem_pool.is_none() {
        return Err(method_not_found());
    }

    let tx = tx.0;
    let raw_block = ctx.store.get_last_valid_tip_block()?.raw();
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
        let state = ctx.mem_pool_state.load_state_db();
        if let Err(err) = verify_sender_balance(ctx.generator.rollup_context(), &state, &tx.raw()) {
            return Err(rpc_error(
                ErrorCode::InvalidRequest,
                format!("check balance err: {}", err),
            ));
        }
    }

    let execution_span = tracing::info_span!("execution");
    let mut run_result = tokio::task::spawn_blocking(move || {
        let _entered = execution_span.entered();

        let db = ctx.store.get_snapshot();
        let tip_block_hash = db.get_last_valid_tip_block_hash()?;
        let chain_view = ChainView::new(&db, tip_block_hash);
        let mut state = ctx.mem_pool_state.load_state_db();
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
        TransactionVerifier::new(
            &state,
            ctx.generator.rollup_context(),
            polyjuice_creator_id,
            ctx.generator.fork_config(),
        )
        .verify(&tx, block_info.number().unpack())?;
        // verify tx signature
        ctx.generator.check_transaction_signature(&state, &tx)?;
        // execute tx
        let raw_tx = tx.raw();
        let run_result = ctx.generator.execute_transaction(
            &chain_view,
            &mut state,
            &block_info,
            &raw_tx,
            Some(ctx.mem_pool_config.execute_l2tx_max_cycles),
            Some(&mut cycles_pool),
        )?;

        anyhow::Ok(run_result)
    })
    .await??;
    gw_metrics::rpc()
        .execute_transactions(run_result.exit_code)
        .inc();

    if run_result.exit_code != 0 {
        let receipt = gw_types::offchain::ErrorTxReceipt {
            tx_hash,
            block_number: number,
            return_data: run_result.return_data,
            last_log: run_result.logs.pop(),
            exit_code: run_result.exit_code,
        };

        return Err(rpc_error_with_data(
            ErrorCode::InvalidRequest,
            TransactionError::InvalidExitCode(run_result.exit_code).to_string(),
            ErrorTxReceipt::from(receipt),
        ));
    }

    Ok(run_result.into())
}

#[instrument(skip_all)]
async fn gw_execute_raw_l2transaction(
    ctx: Arc<Registry>,
    raw_l2tx: RawL2TransactionJsonBytes,
    block_number_opt: Option<Uint64>,
    registry_address_opt: Option<RegistryAddressJsonBytes>,
) -> Result<RunResult> {
    let block_number_opt = block_number_opt.map(|n| n.value());
    let raw_l2tx = raw_l2tx.0;
    let registry_address_opt = registry_address_opt.map(|r| r.0);

    let mut db_txn = ctx.store.begin_transaction();

    let block_info = match block_number_opt {
        Some(block_number) => {
            let db = &db_txn;
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
        None => ctx
            .mem_pool_state
            .get_mem_pool_block_info()
            .expect("get mem pool block info"),
    };

    let execute_l2tx_max_cycles = ctx.mem_pool_config.execute_l2tx_max_cycles;
    let tx_hash: H256 = raw_l2tx.hash();
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
                let state =
                    BlockStateDB::from_store(&mut db_txn, RWConfig::history_block(block_number))?;
                verify_sender_balance(ctx.generator.rollup_context(), &state, &raw_l2tx)
            }
            None => {
                let state = ctx.mem_pool_state.load_state_db();
                verify_sender_balance(ctx.generator.rollup_context(), &state, &raw_l2tx)
            }
        };
        if let Err(err) = check_balance_result {
            return Err(rpc_error(
                ErrorCode::InvalidRequest,
                format!("check balance err: {}", err),
            ));
        }
    }

    // execute tx in task
    let execution_span = tracing::info_span!("execution");
    let mut run_result = tokio::task::spawn_blocking(move || {
        let _entered = execution_span.entered();

        let eth_recover = &ctx.polyjuice_sender_recover.eth;
        let rollup_context = ctx.generator.rollup_context();
        let snap = db_txn.snapshot();
        let chain_view = {
            let tip_block_hash = snap.get_last_valid_tip_block_hash()?;
            ChainView::new(&snap, tip_block_hash)
        };
        // execute tx
        let run_result = match block_number_opt {
            Some(block_number) => {
                let mut state =
                    BlockStateDB::from_store(&mut db_txn, RWConfig::history_block(block_number))?;
                let raw_l2tx = eth_recover.mock_sender_if_not_exists_from_raw_registry(
                    raw_l2tx,
                    registry_address_opt,
                    &mut state,
                )?;
                if 0 == from_id {
                    verify_sender_balance(rollup_context, &state, &raw_l2tx)
                        .map_err(|err| anyhow!("check balance err {}", err))?;
                }

                ctx.generator.execute_transaction(
                    &chain_view,
                    &mut state,
                    &block_info,
                    &raw_l2tx,
                    Some(execute_l2tx_max_cycles),
                    Some(&mut cycles_pool),
                )?
            }
            None => {
                let mut state = ctx.mem_pool_state.load_state_db();
                let raw_l2tx = eth_recover.mock_sender_if_not_exists_from_raw_registry(
                    raw_l2tx,
                    registry_address_opt,
                    &mut state,
                )?;
                if 0 == from_id {
                    verify_sender_balance(rollup_context, &state, &raw_l2tx)
                        .map_err(|err| anyhow!("check balance err {}", err))?;
                }

                ctx.generator.execute_transaction(
                    &chain_view,
                    &mut state,
                    &block_info,
                    &raw_l2tx,
                    Some(execute_l2tx_max_cycles),
                    Some(&mut cycles_pool),
                )?
            }
        };
        anyhow::Ok(run_result)
    })
    .await??;
    gw_metrics::rpc()
        .execute_transactions(run_result.exit_code)
        .inc();

    if run_result.exit_code != 0 {
        let receipt = gw_types::offchain::ErrorTxReceipt {
            tx_hash,
            block_number,
            return_data: run_result.return_data,
            last_log: run_result.logs.pop(),
            exit_code: run_result.exit_code,
        };
        return Err(rpc_error_with_data(
            ErrorCode::InvalidRequest,
            TransactionError::InvalidExitCode(run_result.exit_code).to_string(),
            ErrorTxReceipt::from(receipt),
        ));
    }

    Ok(run_result.into())
}

#[instrument(skip_all)]
async fn gw_submit_l2transaction(
    ctx: &Registry,
    l2tx: L2TransactionJsonBytes,
) -> Result<Option<JsonH256>> {
    let tx = l2tx.0;
    let tx_hash: H256 = tx.hash();

    let sender_id: u32 = tx.raw().from_id().unpack();
    let eth_recover = &ctx.polyjuice_sender_recover.eth;
    if 0 == sender_id && eth_recover.opt_account_creator.is_none() {
        return Err("tx from zero is disabled".into());
    }

    // Return None for tx from zero because its from id will be updated after account creation.
    let tx_hash_json = if 0 == sender_id {
        None
    } else {
        Some(to_jsonh256(tx.hash()))
    };

    // check rate limit
    if let Some(ref rate_limiter) = ctx.send_tx_rate_limit {
        let mut rate_limiter = rate_limiter.lock().await;
        let sender_id: u32 = tx.raw().from_id().unpack();
        if let Some(last_touch) = rate_limiter.get(&sender_id) {
            if last_touch.elapsed().as_secs()
                < ctx
                    .send_tx_rate_limit_config
                    .as_ref()
                    .map(|c| c.seconds)
                    .unwrap_or_default()
            {
                return Err("Rate limit, please wait few seconds and try again".into());
            }
        }
        rate_limiter.put(sender_id, Instant::now());
    }

    // TODO use TransactionVerifier after remove sender auto creator
    // verify tx size
    {
        // block info
        let block_info = ctx
            .mem_pool_state
            .load_shared()
            .mem_block
            .expect("mem block info");
        // check tx size
        let max_tx_size = ctx
            .generator
            .fork_config()
            .max_tx_size(block_info.number().unpack());
        if tx.as_slice().len() > max_tx_size {
            let err = TransactionError::ExceededMaxTxSize {
                max_size: max_tx_size,
                tx_size: tx.as_slice().len(),
            };
            return Err(rpc_error(ErrorCode::InvalidRequest, err.to_string()));
        }
    }

    // check sender's nonce
    {
        // fetch mem-pool state
        let state = ctx.mem_pool_state.load_state_db();

        let tx_nonce: u32 = tx.raw().nonce().unpack();
        let sender_nonce: u32 = if 0 == sender_id {
            0
        } else {
            state.get_nonce(sender_id)?
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
            return Err(rpc_error(INVALID_NONCE_ERR_CODE, err.to_string()));
        }
    }

    let permit = ctx.submit_tx.try_reserve().map_err(|err| match err {
        mpsc::error::TrySendError::Full(_) => rpc_error(BUSY_ERR_CODE, "mem pool service busy"),
        e => e.into(),
    })?;

    let tx_hash_in_queue = match tx_hash_json {
        Some(_) => tx_hash,
        None => {
            let mut hasher = new_blake2b();
            let sig: Bytes = tx.signature().unpack();
            hasher.update(&sig);
            let mut hash = [0u8; 32];
            hasher.finalize(&mut hash);
            hash
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
        let in_queue_span = tracing::info_span!("submit_queue.send");
        let _entered = in_queue_span.clone().entered();
        let ctx = RequestContext {
            _in_queue_handle: handle,
            trace: gw_telemetry::current_context(),
            in_queue_span,
        };
        permit.send((request, ctx));
    }

    Ok(tx_hash_json)
}

#[instrument(skip_all)]
async fn gw_submit_withdrawal_request(
    ctx: &Registry,
    withdrawal: WithdrawalRequestExtraJsonBytes,
) -> Result<JsonH256> {
    let withdrawal = withdrawal.0;
    let withdrawal_hash = withdrawal.hash();

    let last_valid = ctx.store.get_last_valid_tip_block_hash()?;
    let last_valid = ctx
        .store
        .get_block_number(&last_valid)?
        .expect("tip block number");
    let finalized_custodians = ctx
        .store
        .get_block_post_finalized_custodian_capacity(last_valid)
        .expect("finalized custodians");
    let withdrawal_generator = gw_mem_pool::withdrawal::Generator::new(
        ctx.generator.rollup_context(),
        finalized_custodians.as_reader().unpack(),
    );
    if let Err(err) = withdrawal_generator.verify_remained_amount(&withdrawal.request()) {
        return Err(rpc_error(
            CUSTODIAN_NOT_ENOUGH_CODE,
            format!(
                "Withdrawal fund are still finalizing, please try again later. error: {}",
                err
            ),
        ));
    }
    if let Err(err) = withdrawal_generator.verified_output(&withdrawal, &Default::default()) {
        return Err(rpc_error(ErrorCode::InvalidRequest, err.to_string()));
    }

    let permit = ctx.submit_tx.try_reserve().map_err(|err| match err {
        mpsc::error::TrySendError::Full(_) => rpc_error(BUSY_ERR_CODE, "mem pool service busy"),
        e => e.into(),
    })?;

    let request = Request::Withdrawal(withdrawal);
    // Use permit to insert before send so that remove won't happen before insert.
    if let Some(handle) = ctx
        .in_queue_request_map
        .as_ref()
        .expect("in_queue_request_map")
        .insert(withdrawal_hash, request.clone())
    {
        // Send if the request wasn't already in the map.
        let in_queue_span = tracing::info_span!("submit_queue.send");
        let _entered = in_queue_span.clone().entered();
        let ctx = RequestContext {
            _in_queue_handle: handle,
            trace: gw_telemetry::current_context(),
            in_queue_span,
        };
        permit.send((request, ctx));
    }

    Ok(withdrawal_hash.into())
}

#[instrument(skip_all)]
async fn gw_get_withdrawal(
    ctx: &Registry,
    withdrawal_hash: JsonH256,
    verbose: Option<GetVerbose>,
) -> Result<Option<WithdrawalWithStatus>> {
    let withdrawal_hash = withdrawal_hash.into();
    let verbose = verbose.unwrap_or_default();

    if let Some(w) = ctx
        .in_queue_request_map
        .as_deref()
        .and_then(|m| m.get_withdrawal(&withdrawal_hash))
    {
        return Ok(Some(WithdrawalWithStatus {
            withdrawal: verbose.verbose().then(|| w.into()),
            status: WithdrawalStatus::Pending,
            ..Default::default()
        }));
    }
    let db = ctx.store.get_snapshot();
    if let Some(withdrawal) = db.get_mem_pool_withdrawal(&withdrawal_hash)? {
        let withdrawal_opt = verbose.verbose().then(|| withdrawal.into());
        return Ok(Some(WithdrawalWithStatus {
            status: WithdrawalStatus::Pending,
            withdrawal: withdrawal_opt,
            ..Default::default()
        }));
    }
    if let Some(withdrawal_info) = db.get_withdrawal_info(&withdrawal_hash)? {
        if let Some(withdrawal) = db.get_withdrawal_by_key(&withdrawal_info.key())? {
            let withdrawal_opt = verbose.verbose().then(|| withdrawal.into());
            let l2_block_number: u64 = withdrawal_info.block_number().unpack();
            let l2_block_hash = withdrawal_info.key().as_slice()[..32].try_into().unwrap();
            let l2_withdrawal_index: u32 =
                packed::Uint32Reader::from_slice(&withdrawal_info.key().as_slice()[32..36])
                    .unwrap()
                    .unpack();
            let l2_committed_info = Some(L2WithdrawalCommittedInfo {
                block_number: l2_block_number.into(),
                block_hash: to_jsonh256(l2_block_hash),
                withdrawal_index: l2_withdrawal_index.into(),
            });
            let l1_committed_info = gw_get_block_committed_info(l2_block_hash.into(), ctx).await?;
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

#[instrument(skip_all)]
async fn gw_get_balance(
    ctx: &Registry,
    address: RegistryAddressJsonBytes,
    sudt_id: AccountID,
    block_number: Option<Uint64>,
) -> Result<U256> {
    let address = address.0;
    let balance = match block_number {
        Some(block_number) => {
            let mut db = ctx.store.begin_transaction();
            let tree =
                BlockStateDB::from_store(&mut db, RWConfig::history_block(block_number.into()))?;
            tree.get_sudt_balance(sudt_id.into(), &address)?
        }
        None => {
            let state = ctx.mem_pool_state.load_state_db();
            state.get_sudt_balance(sudt_id.into(), &address)?
        }
    };
    Ok(balance)
}

#[instrument(skip_all)]
async fn gw_get_storage_at(
    ctx: &Registry,
    account_id: AccountID,
    key: JsonH256,
    block_number: Option<Uint64>,
) -> Result<JsonH256> {
    let value = match block_number {
        Some(block_number) => {
            let mut db = ctx.store.begin_transaction();
            let tree =
                BlockStateDB::from_store(&mut db, RWConfig::history_block(block_number.into()))?;
            let key: H256 = to_h256(key);
            tree.get_value(account_id.into(), key.as_slice())?
        }
        None => {
            let state = ctx.mem_pool_state.load_state_db();
            let key: H256 = to_h256(key);
            state.get_value(account_id.into(), key.as_slice())?
        }
    };

    let json_value = to_jsonh256(value);
    Ok(json_value)
}

#[instrument(skip_all)]
async fn gw_get_account_id_by_script_hash(
    ctx: &Registry,
    script_hash: JsonH256,
) -> Result<Option<AccountID>> {
    let state = ctx.mem_pool_state.load_state_db();

    let script_hash = to_h256(script_hash);

    let account_id_opt = state
        .get_account_id_by_script_hash(&script_hash)?
        .map(Into::into);

    Ok(account_id_opt)
}

#[instrument(skip_all)]
async fn gw_get_nonce(
    ctx: &Registry,
    account_id: AccountID,
    block_number: Option<Uint64>,
) -> Result<Uint32> {
    let nonce = match block_number {
        Some(block_number) => {
            let mut db = ctx.store.begin_transaction();
            let tree =
                BlockStateDB::from_store(&mut db, RWConfig::history_block(block_number.into()))?;
            tree.get_nonce(account_id.into())?
        }
        None => {
            let state = ctx.mem_pool_state.load_state_db();
            state.get_nonce(account_id.into())?
        }
    };

    Ok(nonce.into())
}

#[instrument(skip_all)]
async fn gw_get_script(ctx: &Registry, script_hash: JsonH256) -> Result<Option<Script>> {
    let state = ctx.mem_pool_state.load_state_db();

    let script_hash = to_h256(script_hash);
    let script_opt = state.get_script(&script_hash).map(Into::into);

    Ok(script_opt)
}

#[instrument(skip_all)]
async fn gw_get_script_hash(ctx: &Registry, account_id: AccountID) -> Result<JsonH256> {
    let state = ctx.mem_pool_state.load_state_db();
    let script_hash = state.get_script_hash(account_id.into())?;
    Ok(to_jsonh256(script_hash))
}

#[instrument(skip_all)]
async fn gw_get_script_hash_by_registry_address(
    ctx: &Registry,
    address: RegistryAddressJsonBytes,
) -> Result<Option<JsonH256>> {
    let state = ctx.mem_pool_state.load_state_db();
    let addr = address.0;
    let script_hash_opt = state.get_script_hash_by_registry_address(&addr)?;
    Ok(script_hash_opt.map(to_jsonh256))
}

#[instrument(skip_all)]
async fn gw_get_registry_address_by_script_hash(
    ctx: &Registry,
    script_hash: JsonH256,
    registry_id: Uint32,
) -> Result<Option<RegistryAddress>> {
    let state = ctx.mem_pool_state.load_state_db();
    let addr =
        state.get_registry_address_by_script_hash(registry_id.value(), &to_h256(script_hash))?;
    Ok(addr.map(Into::into))
}

fn get_backend_info(generator: Arc<Generator>) -> Vec<BackendInfo> {
    generator
        .backend_manage()
        .get_block_consensus_at_height(0)
        .expect("backends")
        .1
        .backends
        .values()
        .map(|b| BackendInfo {
            validator_code_hash: ckb_fixed_hash::H256(b.checksum.validator),
            generator_code_hash: ckb_fixed_hash::H256(b.checksum.generator),
            validator_script_type_hash: ckb_fixed_hash::H256(b.validator_script_type_hash),
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
    let required_staking_capacity: Uint64 = rollup_config
        .required_staking_capacity()
        .as_reader()
        .unpack()
        .into();
    let challenge_maturity_blocks: Uint64 = rollup_config
        .challenge_maturity_blocks()
        .as_reader()
        .unpack()
        .into();
    let finality_blocks: Uint64 = rollup_config.finality_blocks().as_reader().unpack().into();
    let burn_rate: u32 = u8::from(rollup_config.reward_burn_rate()).into();
    let reward_burn_rate: Uint32 = burn_rate.into();
    let chain_id: Uint64 = rollup_config.chain_id().as_reader().unpack().into();
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
