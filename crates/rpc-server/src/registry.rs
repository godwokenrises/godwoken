use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use ckb_types::prelude::{Builder, Entity};
use gw_common::{blake2b::new_blake2b, state::State, H256};
use gw_config::{
    FeeConfig, MemPoolConfig, NodeMode, RPCMethods, RPCRateLimit, RPCServerConfig,
    WithdrawalToV1Config,
};
use gw_dynamic_config::manager::{DynamicConfigManager, DynamicConfigReloadResponse};
use gw_generator::{error::TransactionError, sudt::build_l2_sudt_script, ArcSwap, Generator};
use gw_jsonrpc_types::{
    blockchain::Script,
    ckb_jsonrpc_types::{JsonBytes, Uint128, Uint32},
    godwoken::{
        BackendInfo, ErrorTxReceipt, GlobalState, L2BlockCommittedInfo, L2BlockStatus, L2BlockView,
        L2BlockWithStatus, L2TransactionStatus, L2TransactionWithStatus, LastL2BlockCommittedInfo,
        NodeInfo, RunResult, SUDTFeeConfig, TxReceipt, WithdrawalStatus, WithdrawalWithStatus,
    },
    test_mode::{ShouldProduceBlock, TestModePayload},
};
use gw_mem_pool::{
    custodian::AvailableCustodians,
    fee::{
        queue::FeeQueue,
        types::{FeeEntry, FeeItem},
    },
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
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{
        self, BlockInfo, Byte32, L2Transaction, RollupConfig, V1DepositLockArgsReader,
        WithdrawalRequestExtra,
    },
    prelude::*,
};
use gw_utils::since::{LockValue, Since};
use gw_version::Version;
use jsonrpc_v2::{Data, Error as RpcError, MapRouter, Params, Server, Server as JsonrpcServer};
use lru::LruCache;
use once_cell::sync::Lazy;
use pprof::ProfilerGuard;
use std::{
    convert::{TryFrom, TryInto},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use tracing::instrument;

static PROFILER_GUARD: Lazy<tokio::sync::Mutex<Option<ProfilerGuard>>> =
    Lazy::new(|| tokio::sync::Mutex::new(None));

// type alias
type RPCServer = Arc<Server<MapRouter>>;
type MemPool = Option<Arc<Mutex<gw_mem_pool::pool::MemPool>>>;
type AccountID = Uint32;
type JsonH256 = ckb_fixed_hash::H256;
type BoxedTestsRPCImpl = Box<dyn TestModeRPC + Send + Sync>;
type GwUint64 = gw_jsonrpc_types::ckb_jsonrpc_types::Uint64;

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
    async fn should_produce_block(&self) -> Result<ShouldProduceBlock>;
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
    dynamic_config: Arc<ArcSwap<DynamicConfigManager>>,
}

pub struct SubmitWithdrawalContext {
    generator: Arc<Generator>,
    store: Store,
    submit_tx: async_channel::Sender<Request>,
    rpc_client: RPCClient,
    to_v1_verifier: WithdrawalToV1RequestVerifier,
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
    pub dynamic_config_manager: Arc<ArcSwap<DynamicConfigManager>>,
    pub last_submitted_tx_hash: Option<Arc<tokio::sync::RwLock<H256>>>,
    pub withdrawal_to_v1_config: Option<WithdrawalToV1Config>,
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
    submit_tx: async_channel::Sender<Request>,
    rpc_client: RPCClient,
    send_tx_rate_limit: Option<RPCRateLimit>,
    server_config: RPCServerConfig,
    dynamic_config_manager: Arc<ArcSwap<DynamicConfigManager>>,
    last_submitted_tx_hash: Option<Arc<tokio::sync::RwLock<H256>>>,
    mem_pool_state: Arc<MemPoolState>,
    withdrawal_to_v1_verifier: WithdrawalToV1RequestVerifier,
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
            dynamic_config_manager,
            last_submitted_tx_hash,
            withdrawal_to_v1_config,
        } = args;

        let backend_info = get_backend_info(generator.clone());

        let mem_pool_state = match mem_pool.as_ref() {
            Some(pool) => {
                let mem_pool = pool.lock().await;
                mem_pool.mem_pool_state()
            }
            None => Arc::new(MemPoolState::new(Arc::new(MemStore::new(
                store.get_snapshot(),
            )))),
        };
        let (submit_tx, submit_rx) = async_channel::bounded(RequestSubmitter::MAX_CHANNEL_SIZE);
        if let Some(mem_pool) = mem_pool.as_ref().to_owned() {
            let submitter = RequestSubmitter {
                mem_pool: Arc::clone(mem_pool),
                submit_rx,
                queue: Arc::new(Mutex::new(FeeQueue::new())),
                dynamic_config_manager: dynamic_config_manager.clone(),
                generator: generator.clone(),
                mem_pool_state: mem_pool_state.clone(),
                store: store.clone(),
            };
            tokio::spawn(submitter.in_background());
        }
        let withdrawal_to_v1_verifier = WithdrawalToV1RequestVerifier::new(withdrawal_to_v1_config);

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
            dynamic_config_manager,
            last_submitted_tx_hash,
            mem_pool_state,
            withdrawal_to_v1_verifier,
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
                dynamic_config: self.dynamic_config_manager.clone(),
            }))
            .with_data(Data::new(SubmitWithdrawalContext {
                generator: self.generator.clone(),
                store: self.store.clone(),
                submit_tx: self.submit_tx.clone(),
                rpc_client: self.rpc_client.clone(),
                to_v1_verifier: self.withdrawal_to_v1_verifier.clone(),
            }))
            .with_data(Data::new(self.mem_pool))
            .with_data(Data(self.generator.clone()))
            .with_data(Data::new(self.store))
            .with_data(Data::new(self.rollup_config))
            .with_data(Data::new(self.mem_pool_config))
            .with_data(Data::new(self.backend_info))
            .with_data(Data::new(self.submit_tx))
            .with_data(Data::new(self.rpc_client))
            .with_data(Data::new(self.send_tx_rate_limit))
            .with_data(Data::new(send_transaction_rate_limiter))
            .with_data(Data::new(self.dynamic_config_manager.clone()))
            .with_data(Data::new(self.mem_pool_state))
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
                "gw_get_script_hash_by_short_address",
                get_script_hash_by_short_address,
            )
            .with_method("gw_get_data", get_data)
            .with_method("gw_get_transaction", get_transaction)
            .with_method("gw_get_transaction_receipt", get_transaction_receipt)
            .with_method("gw_get_withdrawal", get_withdrawal)
            .with_method("gw_execute_l2transaction", execute_l2transaction)
            .with_method("gw_execute_raw_l2transaction", execute_raw_l2transaction)
            .with_method(
                "gw_compute_l2_sudt_script_hash",
                compute_l2_sudt_script_hash,
            )
            .with_method("gw_get_fee_config", get_fee_config)
            .with_method("gw_get_mem_pool_state_root", get_mem_pool_state_root)
            .with_method("gw_get_node_info", get_node_info)
            .with_method("gw_reload_config", reload_config);

        if self.node_mode != NodeMode::ReadOnly {
            server = server
                .with_method("gw_submit_l2transaction", submit_l2transaction)
                .with_method("gw_submit_withdrawal_request", submit_withdrawal_request);
        }

        if let Some(last_submitted_tx_hash) = self.last_submitted_tx_hash {
            server = server
                .with_data(Data(last_submitted_tx_hash))
                .with_method("gw_get_last_submitted_info", get_last_submitted_info);
        }

        // Tests
        if let Some(tests_rpc_impl) = self.tests_rpc_impl {
            server = server
                .with_data(Data(Arc::clone(&tests_rpc_impl)))
                .with_method("tests_produce_block", tests_produce_block)
                .with_method("tests_should_produce_block", tests_should_produce_block)
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
                        .with_method("gw_dump_jemalloc_profiling", dump_jemalloc_profiling);
                }
            }
        }

        Ok(server.finish())
    }
}

enum Request {
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
    submit_rx: async_channel::Receiver<Request>,
    queue: Arc<Mutex<FeeQueue>>,
    dynamic_config_manager: Arc<ArcSwap<DynamicConfigManager>>,
    generator: Arc<Generator>,
    mem_pool_state: Arc<MemPoolState>,
    store: Store,
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
                .load_backend(state, &script_hash)
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

    async fn in_background(self) {
        // First mem pool reinject txs
        {
            let db = self.store.begin_transaction();
            let mut mem_pool = self.mem_pool.lock().await;

            log::info!(
                "reinject mem block txs {}",
                mem_pool.pending_restored_tx_hashes().len()
            );
            while let Some(hash) = mem_pool.pending_restored_tx_hashes().pop_front() {
                match db.get_mem_pool_transaction(&hash) {
                    Ok(Some(tx)) => {
                        if let Err(err) = mem_pool.push_transaction(tx).await {
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
        }

        loop {
            // check mem block empty slots
            loop {
                log::debug!("[Mem-pool background job] check mem-pool acquire mem_pool",);
                let t = Instant::now();
                let mem_pool = self.mem_pool.lock().await;
                log::debug!(
                    "[Mem-pool background job] check-mem-pool unlock mem_pool {}ms",
                    t.elapsed().as_millis()
                );
                // continue to batch process if we have enough mem block slots
                if !mem_pool.is_mem_txs_full(Self::MAX_BATCH_SIZE) {
                    break;
                }
                drop(mem_pool);
                // sleep and try again
                tokio::time::sleep(Self::INTERVAL_MS).await;
            }

            // mem-pool can process more txs
            let mut queue = self.queue.lock().await;

            // wait next tx if queue is empty
            if queue.is_empty() {
                // blocking current task until we receive a tx
                let req = match self.submit_rx.recv().await {
                    Ok(req) => req,
                    Err(_) if self.submit_rx.is_closed() => {
                        log::error!("rpc submit tx is closed");
                        return;
                    }
                    Err(err) => {
                        log::debug!("rpc submit rx err {}", err);
                        tokio::time::sleep(Self::INTERVAL_MS).await;
                        continue;
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
                        queue.add(entry);
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
            while let Ok(req) = self.submit_rx.try_recv() {
                let kind = req.kind();
                let hash = req.hash();
                let dynamic_config_manager = self.dynamic_config_manager.load();
                let fee_config = dynamic_config_manager.get_fee_config();
                match req_to_entry(fee_config, self.generator.clone(), req, &state, queue.len()) {
                    Ok(entry) => {
                        queue.add(entry);
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
            // release lock
            drop(queue);

            if !items.is_empty() {
                log::debug!("[Mem-pool background job] acquire mem_pool",);
                let t = Instant::now();
                let mut mem_pool = self.mem_pool.lock().await;
                log::debug!(
                    "[Mem-pool background job] unlock mem_pool {}ms",
                    t.elapsed().as_millis()
                );
                for entry in items {
                    let maybe_ok = match entry.item.clone() {
                        FeeItem::Tx(tx) => mem_pool.push_transaction(tx).await,
                        FeeItem::Withdrawal(withdrawal) => {
                            mem_pool.push_withdrawal_request(withdrawal).await
                        }
                    };

                    if let Err(err) = maybe_ok {
                        let hash: Byte32 = entry.item.hash().pack();
                        log::info!("push {:?} {} failed {}", entry.item.kind(), hash, err);
                    }
                }
            }
        }
    }
}

fn get_backend_info(generator: Arc<Generator>) -> Vec<BackendInfo> {
    generator
        .get_backends()
        .values()
        .map(|b| {
            let mut validator_code_hash = [0u8; 32];
            let mut hasher = new_blake2b();
            hasher.update(&b.validator);
            hasher.finalize(&mut validator_code_hash);
            let mut generator_code_hash = [0u8; 32];
            let mut hasher = new_blake2b();
            hasher.update(&b.generator);
            hasher.finalize(&mut generator_code_hash);
            BackendInfo {
                validator_code_hash: validator_code_hash.into(),
                generator_code_hash: generator_code_hash.into(),
                validator_script_type_hash: ckb_fixed_hash::H256(
                    b.validator_script_type_hash.into(),
                ),
            }
        })
        .collect()
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

async fn get_block_committed_info(
    Params((block_hash,)): Params<(JsonH256,)>,
    store: Data<Store>,
) -> Result<Option<L2BlockCommittedInfo>> {
    let block_hash = to_h256(block_hash);
    let db = store.get_snapshot();
    let committed_info = match db.get_l2block_committed_info(&block_hash)? {
        Some(committed_info) => committed_info,
        None => return Ok(None),
    };

    Ok(Some(committed_info.into()))
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
        let tip_block_number = db.get_last_valid_tip_block()?.raw().number().unpack();
        let block_number = block.raw().number().unpack();
        if tip_block_number >= block_number + rollup_config.finality_blocks().unpack() {
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
    store: Data<Store>,
) -> Result<Option<L2BlockView>> {
    let block_number = block_number.value();
    let db = store.get_snapshot();
    let block_hash = match db.get_block_hash_by_number(block_number)? {
        Some(hash) => hash,
        None => return Ok(None),
    };
    let block_opt = db.get_block(&block_hash)?.map(|block| {
        let block_view: L2BlockView = block.into();
        block_view
    });
    Ok(block_opt)
}

async fn get_block_hash(
    Params((block_number,)): Params<(gw_jsonrpc_types::ckb_jsonrpc_types::Uint64,)>,
    store: Data<Store>,
) -> Result<Option<JsonH256>> {
    let block_number = block_number.value();
    let db = store.get_snapshot();
    let hash_opt = db.get_block_hash_by_number(block_number)?.map(to_jsonh256);
    Ok(hash_opt)
}

async fn get_tip_block_hash(store: Data<Store>) -> Result<JsonH256> {
    let tip_block_hash = store.get_snapshot().get_last_valid_tip_block_hash()?;
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
    let block_producer_id = raw_block.block_producer_id();
    let timestamp = raw_block.timestamp();
    let number = {
        let number: u64 = raw_block.number().unpack();
        number.saturating_add(1)
    };

    let block_info = BlockInfo::new_builder()
        .block_producer_id(block_producer_id)
        .timestamp(timestamp)
        .number(number.pack())
        .build();

    let tx_hash = tx.hash();
    let mut run_result = tokio::task::spawn_blocking(move || {
        let db = ctx.store.get_snapshot();
        let tip_block_hash = db.get_last_valid_tip_block_hash()?;
        let chain_view = ChainView::new(&db, tip_block_hash);
        let snap = ctx.mem_pool_state.load();
        let state = snap.state()?;
        // verify tx signature
        ctx.generator.check_transaction_signature(&state, &tx)?;
        // tx basic verification
        ctx.generator.verify_transaction(&state, &tx)?;
        // execute tx
        let raw_tx = tx.raw();
        let run_result = ctx.generator.unchecked_execute_transaction(
            &chain_view,
            &state,
            &block_info,
            &raw_tx,
            100000000,
            Some(ctx.dynamic_config.clone()),
        )?;

        Result::<_, anyhow::Error>::Ok(run_result)
    })
    .await??;

    if run_result.exit_code != 0 {
        let receipt = gw_types::offchain::ErrorTxReceipt {
            tx_hash: tx_hash.into(),
            block_number: number,
            return_data: run_result.return_data,
            last_log: run_result.logs.pop(),
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
}

#[instrument(skip_all)]
async fn execute_raw_l2transaction(
    Params(params): Params<ExecuteRawL2TransactionParams>,
    mem_pool_config: Data<MemPoolConfig>,
    ctx: Data<ExecutionTransactionContext>,
) -> Result<RunResult, RpcError> {
    let (raw_l2tx, block_number_opt) = match params {
        ExecuteRawL2TransactionParams::Tip(p) => (p.0, None),
        ExecuteRawL2TransactionParams::Number(p) => p,
    };
    let block_number_opt = block_number_opt.map(|n| n.value());

    let raw_l2tx_bytes = raw_l2tx.into_bytes();
    let raw_l2tx = packed::RawL2Transaction::from_slice(&raw_l2tx_bytes)?;

    let db = ctx.store.begin_transaction();

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
            let block_producer_id = raw_block.block_producer_id();
            let timestamp = raw_block.timestamp();
            let number: u64 = raw_block.number().unpack();

            BlockInfo::new_builder()
                .block_producer_id(block_producer_id)
                .timestamp(timestamp)
                .number(number.pack())
                .build()
        }
        None => ctx
            .mem_pool_state
            .load()
            .get_mem_pool_block_info()?
            .expect("get mem pool block info"),
    };

    let execute_l2tx_max_cycles = mem_pool_config.execute_l2tx_max_cycles;
    let tx_hash: H256 = raw_l2tx.hash().into();
    let block_number: u64 = block_info.number().unpack();

    // execute tx in task
    let mut run_result = tokio::task::spawn_blocking(move || {
        let chain_view = {
            let tip_block_hash = db.get_last_valid_tip_block_hash()?;
            ChainView::new(&db, tip_block_hash)
        };
        // execute tx
        let run_result = match block_number_opt {
            Some(block_number) => {
                let state = db.state_tree(StateContext::ReadOnlyHistory(block_number))?;
                ctx.generator.unchecked_execute_transaction(
                    &chain_view,
                    &state,
                    &block_info,
                    &raw_l2tx,
                    execute_l2tx_max_cycles,
                    Some(ctx.dynamic_config.clone()),
                )?
            }
            None => {
                let snap = ctx.mem_pool_state.load();
                let state = snap.state()?;
                ctx.generator.unchecked_execute_transaction(
                    &chain_view,
                    &state,
                    &block_info,
                    &raw_l2tx,
                    execute_l2tx_max_cycles,
                    Some(ctx.dynamic_config.clone()),
                )?
            }
        };
        Result::<_, anyhow::Error>::Ok(run_result)
    })
    .await??;

    if run_result.exit_code != 0 {
        let receipt = gw_types::offchain::ErrorTxReceipt {
            tx_hash,
            block_number,
            return_data: run_result.return_data,
            last_log: run_result.logs.pop(),
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

#[instrument(skip_all)]
async fn submit_l2transaction(
    Params((l2tx,)): Params<(JsonBytes,)>,
    submit_tx: Data<async_channel::Sender<Request>>,
    rate_limiter: Data<Option<SendTransactionRateLimiter>>,
    rate_limit_config: Data<Option<RPCRateLimit>>,
    mem_pool_state: Data<Arc<MemPoolState>>,
) -> Result<JsonH256, RpcError> {
    let l2tx_bytes = l2tx.into_bytes();
    let tx = packed::L2Transaction::from_slice(&l2tx_bytes)?;
    let tx_hash = to_jsonh256(tx.hash().into());

    // check rate limit
    if let Some(rate_limiter) = rate_limiter.as_ref() {
        let mut rate_limiter = rate_limiter.lock().await;
        let sender_id: u32 = tx.raw().from_id().unpack();
        if let Some(last_touch) = rate_limiter.get(&sender_id) {
            if last_touch.elapsed().as_secs()
                < rate_limit_config
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
        let snap = mem_pool_state.load();
        let tree = snap.state()?;
        // sender_id
        let sender_id = tx.raw().from_id().unpack();
        let sender_nonce: u32 = tree.get_nonce(sender_id)?;
        let tx_nonce: u32 = tx.raw().nonce().unpack();
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

    if let Err(err) = submit_tx.try_send(Request::Tx(tx)) {
        if err.is_full() {
            return Err(RpcError::Provided {
                code: BUSY_ERR_CODE,
                message: "mem pool service busy",
            });
        }
        if err.is_closed() {
            return Err(RpcError::Provided {
                code: INTERNAL_ERROR_ERR_CODE,
                message: "internal error, unavailable",
            });
        }
    }

    Ok(tx_hash)
}

#[derive(Clone)]
struct WithdrawalToV1RequestVerifier {
    config: Option<WithdrawalToV1Config>,
}

impl WithdrawalToV1RequestVerifier {
    fn new(config: Option<WithdrawalToV1Config>) -> Self {
        Self { config }
    }

    fn verify(&self, request: &WithdrawalRequestExtra) -> Result<()> {
        if request.withdraw_to_v1() != 1u8.into() {
            return Ok(());
        }
        let config = match self.config.as_ref() {
            Some(config) => config,
            None => bail!("withdrawal to v1 is disabled"),
        };

        let deposit_lock = match request.owner_lock().to_opt() {
            Some(lock) => lock,
            None => bail!("v1 deposit lock not found"),
        };
        if deposit_lock.code_hash() != config.v1_deposit_lock_code_hash.pack() {
            bail!("mismatch v1 deposit lock code hash")
        }
        if deposit_lock.hash_type() != ScriptHashType::Type.into() {
            bail!("mismatch v1 deposit lock hash type");
        }

        let args: Bytes = deposit_lock.args().unpack();
        if args.len() <= 32 {
            bail!("invalid v1 deposit lock args");
        }

        let rollup_type_hash = args.slice(..32);
        if rollup_type_hash.as_ref() != config.v1_rollup_type_hash.0 {
            bail!("mismatch v1 rollup type hash");
        }

        let raw_args = args.slice(32..);
        let deposit_args = match V1DepositLockArgsReader::from_slice(raw_args.as_ref()) {
            Ok(args) => args,
            Err(_err) => bail!("invalid v1 deposit lock args"),
        };
        let cancel_timeout = Since::new(deposit_args.cancel_timeout().unpack());
        if !cancel_timeout.flags_is_valid() || !cancel_timeout.is_relative() {
            bail!("invalid v1 deposit cancel timeout");
        }
        match cancel_timeout.extract_lock_value() {
            Some(LockValue::Timestamp(timestamp))
                if timestamp >= config.v1_deposit_minimal_cancel_timeout_msecs =>
            {
                Ok(())
            }
            _ => bail!("invalid v1 deposit cancel timeout"),
        }
    }
}

#[instrument(skip_all)]
async fn submit_withdrawal_request(
    Params((withdrawal_request,)): Params<(JsonBytes,)>,
    ctx: Data<SubmitWithdrawalContext>,
) -> Result<JsonH256, RpcError> {
    let withdrawal = {
        let bytes = withdrawal_request.into_bytes();
        packed::WithdrawalRequestExtra::from_request_compitable_slice(&bytes)?
    };
    ctx.to_v1_verifier.verify(&withdrawal)?;

    let withdrawal_hash = withdrawal.hash();
    // verify finalized custodian
    {
        let t = Instant::now();
        let finalized_custodians = {
            let db = ctx.store.get_snapshot();
            let tip = db.get_last_valid_tip_block()?;
            // query withdrawals from ckb-indexer
            let last_finalized_block_number = ctx
                .generator
                .rollup_context()
                .last_finalized_block_number(tip.raw().number().unpack());
            gw_mem_pool::custodian::query_finalized_custodians(
                &ctx.rpc_client,
                &db,
                vec![withdrawal.request()].into_iter(),
                ctx.generator.rollup_context(),
                last_finalized_block_number,
            )
            .await?
            .expect_any()
        };
        log::debug!(
            "[submit withdrawal] collected {} finalized custodian cells {}ms",
            finalized_custodians.cells_info.len(),
            t.elapsed().as_millis()
        );
        let available_custodians = AvailableCustodians::from(&finalized_custodians);
        let withdrawal_generator = gw_mem_pool::withdrawal::Generator::new(
            ctx.generator.rollup_context(),
            available_custodians,
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
    }

    if let Err(err) = ctx.submit_tx.try_send(Request::Withdrawal(withdrawal)) {
        if err.is_full() {
            return Err(RpcError::Provided {
                code: BUSY_ERR_CODE,
                message: "mem pool service busy",
            });
        }
        if err.is_closed() {
            return Err(RpcError::Provided {
                code: INTERNAL_ERROR_ERR_CODE,
                message: "internal error, unavailable",
            });
        }
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
    let db = store.get_snapshot();
    let withdrawal_opt;
    let status;
    match db.get_withdrawal_info(&withdrawal_hash)? {
        Some(withdrawal_info) => {
            withdrawal_opt = db.get_withdrawal_by_key(&withdrawal_info.key())?;
            status = WithdrawalStatus::Committed;
        }
        None => {
            withdrawal_opt = db
                .get_mem_pool_withdrawal(&withdrawal_hash)?
                .map(|w| w.request());
            status = WithdrawalStatus::Pending;
        }
    };

    Ok(withdrawal_opt.map(|withdrawal| match verbose {
        GetWithdrawalVerbose::OnlyStatus => WithdrawalWithStatus {
            withdrawal: None,
            status,
        },
        GetWithdrawalVerbose::WithdrawalWithStatus => WithdrawalWithStatus {
            withdrawal: Some(withdrawal.into()),
            status,
        },
    }))
}

// short_address, sudt_id, block_number
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
) -> Result<Uint128, RpcError> {
    let (short_address, sudt_id, block_number) = match params {
        GetBalanceParams::Tip(p) => (p.0, p.1, None),
        GetBalanceParams::Number(p) => p,
    };

    let balance = match block_number {
        Some(block_number) => {
            let db = store.begin_transaction();
            let tree = db.state_tree(StateContext::ReadOnlyHistory(block_number.into()))?;
            tree.get_sudt_balance(sudt_id.into(), short_address.as_bytes())?
        }
        None => {
            let snap = mem_pool_state.load();
            let tree = snap.state()?;
            tree.get_sudt_balance(sudt_id.into(), short_address.as_bytes())?
        }
    };
    Ok(balance.into())
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
            tree.get_value(account_id.into(), &key)?
        }
        None => {
            let snap = mem_pool_state.load();
            let tree = snap.state()?;
            let key: H256 = to_h256(key);
            tree.get_value(account_id.into(), &key)?
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

async fn get_script_hash_by_short_address(
    Params((short_address,)): Params<(JsonBytes,)>,
    mem_pool_state: Data<Arc<MemPoolState>>,
) -> Result<Option<JsonH256>, RpcError> {
    let snap = mem_pool_state.load();
    let tree = snap.state()?;
    let script_hash_opt = tree.get_script_hash_by_short_address(&short_address.into_bytes());
    Ok(script_hash_opt.map(to_jsonh256))
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

async fn get_node_info(backend_info: Data<Vec<BackendInfo>>) -> Result<NodeInfo> {
    Ok(NodeInfo {
        version: Version::current().to_string(),
        backends: backend_info.clone(),
    })
}

async fn get_last_submitted_info(
    last_submitted_tx_hash: Data<tokio::sync::RwLock<H256>>,
) -> Result<LastL2BlockCommittedInfo> {
    Ok(LastL2BlockCommittedInfo {
        transaction_hash: {
            let hash: [u8; 32] = (*last_submitted_tx_hash.read().await).into();
            hash.into()
        },
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
        sudt_fee_rate_weight: fee
            .sudt_fee_rate_weight
            .iter()
            .map(|(&sudt_id, &fee_rate_weight)| SUDTFeeConfig {
                sudt_id,
                fee_rate_weight: fee_rate_weight.into(),
            })
            .collect(),
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

async fn tests_produce_block(
    Params((payload,)): Params<(TestModePayload,)>,
    tests_rpc_impl: Data<BoxedTestsRPCImpl>,
) -> Result<()> {
    tests_rpc_impl.produce_block(payload).await
}

async fn tests_get_global_state(tests_rpc_impl: Data<BoxedTestsRPCImpl>) -> Result<GlobalState> {
    tests_rpc_impl.get_global_state().await
}

async fn tests_should_produce_block(
    tests_rpc_impl: Data<BoxedTestsRPCImpl>,
) -> Result<ShouldProduceBlock> {
    tests_rpc_impl.should_produce_block().await
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ckb_fixed_hash::H256;
    use gw_config::WithdrawalToV1Config;
    use gw_types::bytes::Bytes;
    use gw_types::core::ScriptHashType;
    use gw_types::packed::{
        RawWithdrawalRequest, Script, V1DepositLockArgs, WithdrawalRequest, WithdrawalRequestExtra,
    };
    use gw_types::prelude::{Builder, Entity, Pack, Unpack};

    use super::WithdrawalToV1RequestVerifier;

    const FLAG_SINCE_BLOCK_NUMBER: u64 =
        0b000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000;
    const FLAG_SINCE_RELATIVE: u64 =
        0b1000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000;
    const FLAG_SINCE_TIMESTAMP: u64 =
        0b100_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000;

    fn build_cancel_timeout(timeout_secs: u64) -> u64 {
        FLAG_SINCE_RELATIVE | FLAG_SINCE_TIMESTAMP | timeout_secs
    }

    #[test]
    fn test_withdrawal_to_v1_request_verifier() {
        const ONE_DAY: Duration = Duration::from_secs(24 * 60 * 60);
        const SEVEN_DAYS: Duration = Duration::from_secs(ONE_DAY.as_secs() * 7);

        let config = WithdrawalToV1Config {
            v1_rollup_type_hash: H256([1u8; 32]),
            v1_deposit_lock_code_hash: H256([2u8; 32]),
            v1_eth_lock_code_hash: H256([9u8; 32]),
            v1_deposit_minimal_cancel_timeout_msecs: SEVEN_DAYS.as_millis() as u64,
        };
        let verifier = WithdrawalToV1RequestVerifier::new(Some(config.clone()));

        let deposit_args = V1DepositLockArgs::new_builder()
            .layer2_lock(Script::default())
            .owner_lock_hash([4u8; 32].pack())
            .cancel_timeout(build_cancel_timeout(SEVEN_DAYS.as_secs()).pack())
            .build();
        let lock_args = {
            let mut args = config.v1_rollup_type_hash.0.to_vec();
            args.extend_from_slice(&deposit_args.as_bytes());
            args
        };
        let deposit_lock = Script::new_builder()
            .code_hash(config.v1_deposit_lock_code_hash.pack())
            .hash_type(ScriptHashType::Type.into())
            .args(lock_args.pack())
            .build();

        let raw = RawWithdrawalRequest::new_builder()
            .capacity(u64::MAX.pack())
            .amount(u128::MAX.pack())
            .account_script_hash([3u8; 32].pack())
            .owner_lock_hash(deposit_lock.hash().pack())
            .sudt_script_hash([5u8; 32].pack())
            .build();
        let req = WithdrawalRequest::new_builder().raw(raw.clone()).build();
        let req_extra = WithdrawalRequestExtra::new_builder()
            .request(req.clone())
            .owner_lock(Some(deposit_lock.clone()).pack())
            .withdraw_to_v1(1u8.into())
            .build();

        verifier.verify(&req_extra).expect("valid withdrawal to v1");

        // ## Not withdrawal to v1
        let no_2v1_req = req_extra.clone().as_builder().withdraw_to_v1(0u8.into());
        verifier.verify(&no_2v1_req.build()).expect("valid");

        // ## Disable verifier
        let disabled_verifier = WithdrawalToV1RequestVerifier::new(None);
        let err = disabled_verifier.verify(&req_extra).unwrap_err();
        assert!(err.to_string().contains("withdrawal to v1 is disabled"));

        // ## No deposit lock
        let no_deposit_lock = req_extra.as_builder().owner_lock(None.pack());
        let err = verifier.verify(&no_deposit_lock.build()).unwrap_err();
        assert!(err.to_string().contains("v1 deposit lock not found"));

        let build_error_request = |err_deposit_lock: Script| -> WithdrawalRequestExtra {
            let err_lock_hash = err_deposit_lock.hash().pack();
            let err_raw = raw.clone().as_builder().owner_lock_hash(err_lock_hash);
            let err_req = req.clone().as_builder().raw(err_raw.build()).build();
            WithdrawalRequestExtra::new_builder()
                .request(err_req)
                .owner_lock(Some(err_deposit_lock).pack())
                .withdraw_to_v1(1u8.into())
                .build()
        };

        // ## Deposit lock code hash not match
        let err_deposit_lock = { deposit_lock.clone() }
            .as_builder()
            .code_hash([0u8; 32].pack())
            .build();
        let err_req = build_error_request(err_deposit_lock);
        let err_str = verifier.verify(&err_req).unwrap_err().to_string();
        assert!(err_str.contains("mismatch v1 deposit lock code hash"));

        // ## Deposit lock hash type not match
        let err_deposit_lock = { deposit_lock.clone() }
            .as_builder()
            .hash_type(ScriptHashType::Data.into())
            .build();
        let err_req = build_error_request(err_deposit_lock);
        let err_str = verifier.verify(&err_req).unwrap_err().to_string();
        assert!(err_str.contains("mismatch v1 deposit lock hash type"));

        // ## Args len is smaller than 32
        let err_deposit_lock = { deposit_lock.clone() }
            .as_builder()
            .args([1u8; 32].to_vec().pack())
            .build();
        let err_req = build_error_request(err_deposit_lock);
        let err_str = verifier.verify(&err_req).unwrap_err().to_string();
        assert!(err_str.contains("invalid v1 deposit lock args"));

        // ## V1 rollup type hash not match
        let correct_args_bytes: Bytes = deposit_lock.args().unpack();
        let err_args_bytes = {
            let mut args = vec![0u8; 32];
            args.extend_from_slice(&correct_args_bytes.slice(32..));
            args
        };
        let err_deposit_lock = { deposit_lock.clone() }
            .as_builder()
            .args(err_args_bytes.pack())
            .build();
        let err_req = build_error_request(err_deposit_lock);
        let err_str = verifier.verify(&err_req).unwrap_err().to_string();
        assert!(err_str.contains("mismatch v1 rollup type hash"));

        // ## Invalid V1 deposit lock
        let err_args_bytes = {
            let mut args = config.v1_rollup_type_hash.0.to_vec();
            args.extend_from_slice(&V1DepositLockArgs::default().as_bytes()[1..]);
            args
        };
        let err_deposit_lock = { deposit_lock.clone() }
            .as_builder()
            .args(err_args_bytes.pack())
            .build();
        let err_req = build_error_request(err_deposit_lock);
        let err_str = verifier.verify(&err_req).unwrap_err().to_string();
        assert!(err_str.contains("invalid v1 deposit lock args"));

        // ## Invalid v1 deposit cancel timeout flags
        let err_deposit_args = { deposit_args.clone() }
            .as_builder()
            .cancel_timeout(1u64.pack())
            .build();
        let err_args_bytes = {
            let mut args = config.v1_rollup_type_hash.0.to_vec();
            args.extend_from_slice(&err_deposit_args.as_bytes());
            args
        };
        let err_deposit_lock = { deposit_lock.clone() }
            .as_builder()
            .args(err_args_bytes.pack())
            .build();
        let err_req = build_error_request(err_deposit_lock);
        let err_str = verifier.verify(&err_req).unwrap_err().to_string();
        assert!(err_str.contains("invalid v1 deposit cancel timeout"));

        // ## V1 deposit cancel timeout isn't relative
        let err_deposit_args = { deposit_args.clone() }
            .as_builder()
            .cancel_timeout((FLAG_SINCE_TIMESTAMP | SEVEN_DAYS.as_secs()).pack())
            .build();
        let err_args_bytes = {
            let mut args = config.v1_rollup_type_hash.0.to_vec();
            args.extend_from_slice(&err_deposit_args.as_bytes());
            args
        };
        let err_deposit_lock = { deposit_lock.clone() }
            .as_builder()
            .args(err_args_bytes.pack())
            .build();
        let err_req = build_error_request(err_deposit_lock);
        let err_str = verifier.verify(&err_req).unwrap_err().to_string();
        assert!(err_str.contains("invalid v1 deposit cancel timeout"));

        // ## V1 ceposit cancel timeout is smaller than config
        let smaller_timeout = SEVEN_DAYS.saturating_sub(ONE_DAY);
        let err_deposit_args = { deposit_args.clone() }
            .as_builder()
            .cancel_timeout(build_cancel_timeout(smaller_timeout.as_secs()).pack())
            .build();
        let err_args_bytes = {
            let mut args = config.v1_rollup_type_hash.0.to_vec();
            args.extend_from_slice(&err_deposit_args.as_bytes());
            args
        };
        let err_deposit_lock = { deposit_lock.clone() }
            .as_builder()
            .args(err_args_bytes.pack())
            .build();
        let err_req = build_error_request(err_deposit_lock);
        let err_str = verifier.verify(&err_req).unwrap_err().to_string();
        assert!(err_str.contains("invalid v1 deposit cancel timeout"));

        // ## V1 ceposit cancel timeout use block number
        let err_deposit_args = deposit_args
            .as_builder()
            .cancel_timeout((FLAG_SINCE_RELATIVE | FLAG_SINCE_BLOCK_NUMBER | 1).pack())
            .build();
        let err_args_bytes = {
            let mut args = config.v1_rollup_type_hash.0.to_vec();
            args.extend_from_slice(&err_deposit_args.as_bytes());
            args
        };
        let err_deposit_lock = { deposit_lock.clone() }
            .as_builder()
            .args(err_args_bytes.pack())
            .build();
        let err_req = build_error_request(err_deposit_lock);
        let err_str = verifier.verify(&err_req).unwrap_err().to_string();
        assert!(err_str.contains("invalid v1 deposit cancel timeout"));
    }
}
