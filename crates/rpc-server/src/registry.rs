use anyhow::Result;
use async_trait::async_trait;
use ckb_types::prelude::{Builder, Entity};
use gw_common::{blake2b::new_blake2b, state::State, H256};
use gw_config::{MemPoolConfig, NodeMode, RPCMethods, RPCRateLimit, RPCServerConfig};
use gw_generator::{error::TransactionError, sudt::build_l2_sudt_script, Generator};
use gw_jsonrpc_types::{
    blockchain::Script,
    ckb_jsonrpc_types::{JsonBytes, Uint128, Uint32},
    godwoken::{
        BackendInfo, ErrorTxReceipt, GlobalState, L2BlockCommittedInfo, L2BlockStatus, L2BlockView,
        L2BlockWithStatus, L2TransactionStatus, L2TransactionWithStatus, NodeInfo, RunResult,
        TxReceipt,
    },
    test_mode::{ShouldProduceBlock, TestModePayload},
};
use gw_mem_pool::custodian::AvailableCustodians;
use gw_rpc_client::rpc_client::RPCClient;
// use gw_mem_pool::batch::{MemPoolBatch};
use gw_store::{chain_view::ChainView, state::state_db::StateContext, CfMemStat, Store};
use gw_traits::CodeStore;
use gw_types::{
    packed::{self, BlockInfo, L2Transaction, RawL2Block, RollupConfig, WithdrawalRequest},
    prelude::*,
};
use gw_version::Version;
use jsonrpc_v2::{Data, Error as RpcError, MapRouter, Params, Server, Server as JsonrpcServer};
use lru::LruCache;
use once_cell::sync::Lazy;
use pprof::ProfilerGuard;
use smol::{lock::Mutex, unblock};
use std::{
    convert::{TryFrom, TryInto},
    sync::Arc,
    time::{Duration, Instant},
};

static PROFILER_GUARD: Lazy<std::sync::Mutex<Option<ProfilerGuard>>> =
    Lazy::new(|| std::sync::Mutex::new(None));

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

pub struct Registry {
    generator: Arc<Generator>,
    mem_pool: MemPool,
    store: Store,
    tests_rpc_impl: Option<Arc<BoxedTestsRPCImpl>>,
    rollup_config: RollupConfig,
    mem_pool_config: MemPoolConfig,
    backend_info: Vec<BackendInfo>,
    node_mode: NodeMode,
    submit_tx: smol::channel::Sender<Request>,
    rpc_client: RPCClient,
    send_tx_rate_limit: Option<RPCRateLimit>,
    server_config: RPCServerConfig,
}

impl Registry {
    #[allow(clippy::too_many_arguments)]
    pub fn new<T>(
        store: Store,
        mem_pool: MemPool,
        generator: Arc<Generator>,
        tests_rpc_impl: Option<Box<T>>,
        rollup_config: RollupConfig,
        mem_pool_config: MemPoolConfig,
        node_mode: NodeMode,
        rpc_client: RPCClient,
        send_tx_rate_limit: Option<RPCRateLimit>,
        server_config: RPCServerConfig,
    ) -> Self
    where
        T: TestModeRPC + Send + Sync + 'static,
    {
        let backend_info = get_backend_info(generator.clone());
        let (submit_tx, submit_rx) = smol::channel::bounded(RequestSubmitter::MAX_CHANNEL_SIZE);
        if let Some(mem_pool) = mem_pool.as_ref().to_owned() {
            let submitter = RequestSubmitter {
                mem_pool: Arc::clone(mem_pool),
                submit_rx,
            };
            smol::spawn(submitter.in_background()).detach();
        }

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
        }
    }

    pub fn build_rpc_server(self) -> Result<RPCServer> {
        let mut server = JsonrpcServer::new();

        let send_transaction_rate_limiter: Option<SendTransactionRateLimiter> = self
            .send_tx_rate_limit
            .as_ref()
            .map(|send_tx_rate_limit| Mutex::new(lru::LruCache::new(send_tx_rate_limit.lru_size)));

        server = server
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
            .with_method("gw_execute_l2transaction", execute_l2transaction)
            .with_method("gw_execute_raw_l2transaction", execute_raw_l2transaction)
            .with_method(
                "gw_compute_l2_sudt_script_hash",
                compute_l2_sudt_script_hash,
            )
            .with_method("gw_get_node_info", get_node_info);
        if self.node_mode != NodeMode::ReadOnly {
            server = server
                .with_method("gw_submit_l2transaction", submit_l2transaction)
                .with_method("gw_submit_withdrawal_request", submit_withdrawal_request);
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
    Withdrawal(WithdrawalRequest),
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
    submit_rx: smol::channel::Receiver<Request>,
}

impl RequestSubmitter {
    const MAX_CHANNEL_SIZE: usize = 700;
    const MAX_BATCH_SIZE: usize = 20;
    const INTERVAL_MS: Duration = Duration::from_millis(300);

    async fn in_background(self) {
        loop {
            // check mem block empty slots
            loop {
                if !self.submit_rx.is_empty() {
                    let mem_pool = self.mem_pool.lock().await;
                    // continue to batch process if we have enough mem block slots
                    if !mem_pool.is_mem_txs_full(Self::MAX_BATCH_SIZE) {
                        break;
                    }
                }
                // sleep and try again
                smol::Timer::after(Self::INTERVAL_MS).await;
            }

            let req = match self.submit_rx.recv().await {
                Ok(req) => req,
                Err(_) if self.submit_rx.is_closed() => {
                    log::error!("rpc submit tx is closed");
                    return;
                }
                Err(err) => {
                    log::debug!("rpc submit rx err {}", err);
                    async_std::task::sleep(Self::INTERVAL_MS).await;
                    continue;
                }
            };

            let mut batch = Vec::with_capacity(Self::MAX_BATCH_SIZE);
            batch.push(req);
            while let Ok(req) = self.submit_rx.try_recv() {
                batch.push(req);
                if batch.len() >= Self::MAX_BATCH_SIZE {
                    break;
                }
            }

            if !batch.is_empty() {
                let mut mem_pool = self.mem_pool.lock().await;
                for req in batch.drain(..) {
                    let kind = req.kind();
                    let hash = req.hash();

                    let maybe_ok = match req {
                        Request::Tx(tx) => mem_pool.push_transaction(tx),
                        Request::Withdrawal(withdrawal) => {
                            mem_pool.push_withdrawal_request(withdrawal)
                        }
                    };

                    if let Err(err) = maybe_ok {
                        log::info!("push {} {} failed {}", kind, hash, err);
                    }
                }
            }
            async_std::task::sleep(Self::INTERVAL_MS).await;
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
    let db = store.begin_transaction();
    let tx_opt;
    let status;
    match db.get_transaction_info(&tx_hash)? {
        Some(tx_info) => {
            let tx_block_number = tx_info.block_number().unpack();

            // return None if tx's committed block is reverted
            if !db
                .reverted_block_smt()?
                .get(&RawL2Block::compute_smt_key(tx_block_number).into())?
                .is_zero()
            {
                // block is reverted
                return Ok(None);
            }

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
    let db = store.begin_transaction();
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
    let db = store.begin_transaction();
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
    let db = store.begin_transaction();
    let hash_opt = db.get_block_hash_by_number(block_number)?.map(to_jsonh256);
    Ok(hash_opt)
}

async fn get_tip_block_hash(store: Data<Store>) -> Result<JsonH256> {
    let tip_block_hash = store.get_tip_block_hash()?;
    Ok(to_jsonh256(tip_block_hash))
}

async fn get_transaction_receipt(
    Params((tx_hash,)): Params<(JsonH256,)>,
    store: Data<Store>,
) -> Result<Option<TxReceipt>> {
    let tx_hash = to_h256(tx_hash);
    let db = store.begin_transaction();
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

async fn execute_l2transaction(
    Params((l2tx,)): Params<(JsonBytes,)>,
    mem_pool: Data<MemPool>,
    generator: Data<Generator>,
    store: Data<Store>,
) -> Result<RunResult, RpcError> {
    let _mem_pool = match &*mem_pool {
        Some(mem_pool) => mem_pool,
        None => {
            return Err(mem_pool_is_disabled_err());
        }
    };
    let l2tx_bytes = l2tx.into_bytes();
    let tx = packed::L2Transaction::from_slice(&l2tx_bytes)?;

    let raw_block = store.get_tip_block()?.raw();
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
    let mut run_result = unblock(move || {
        let tip_block_hash = store.get_tip_block_hash()?;
        let db = store.begin_transaction();
        let chain_view = ChainView::new(&db, tip_block_hash);
        let state = db.mem_pool_state_tree()?;
        // verify tx signature
        generator.check_transaction_signature(&state, &tx)?;
        // tx basic verification
        generator.verify_transaction(&state, &tx)?;
        // execute tx
        let raw_tx = tx.raw();
        let run_result = generator.unchecked_execute_transaction(
            &chain_view,
            &state,
            &block_info,
            &raw_tx,
            100000000,
        )?;

        Result::<_, anyhow::Error>::Ok(run_result)
    })
    .await?;

    if run_result.exit_code != 0 {
        let receipt = gw_types::offchain::ErrorTxReceipt {
            tx_hash: tx_hash.into(),
            block_number: number,
            return_data: run_result.return_data,
            last_log: run_result.logs.pop(),
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

async fn execute_raw_l2transaction(
    Params(params): Params<ExecuteRawL2TransactionParams>,
    mem_pool_config: Data<MemPoolConfig>,
    store: Data<Store>,
    generator: Data<Generator>,
) -> Result<RunResult, RpcError> {
    let (raw_l2tx, block_number_opt) = match params {
        ExecuteRawL2TransactionParams::Tip(p) => (p.0, None),
        ExecuteRawL2TransactionParams::Number(p) => p,
    };
    let block_number_opt = block_number_opt.map(|n| n.value());

    let raw_l2tx_bytes = raw_l2tx.into_bytes();
    let raw_l2tx = packed::RawL2Transaction::from_slice(&raw_l2tx_bytes)?;

    let db = store.begin_transaction();

    let block_info = match block_number_opt {
        Some(block_number) => {
            let block_hash = match db.get_block_hash_by_number(block_number)? {
                Some(block_hash) => block_hash,
                None => return Err(header_not_found_err()),
            };
            let raw_block = match store.get_block(&block_hash)? {
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
        None => db
            .get_mem_pool_block_info()?
            .expect("get mem pool block info"),
    };

    let execute_l2tx_max_cycles = mem_pool_config.execute_l2tx_max_cycles;
    let tx_hash: H256 = raw_l2tx.hash().into();
    let block_number: u64 = block_info.number().unpack();

    // execute tx in task
    let mut run_result = unblock(move || {
        let chain_view = {
            let tip_block_hash = db.get_tip_block_hash()?;
            ChainView::new(&db, tip_block_hash)
        };
        // execute tx
        let run_result = match block_number_opt {
            Some(block_number) => {
                let state = db.state_tree(StateContext::ReadOnlyHistory(block_number))?;
                generator.unchecked_execute_transaction(
                    &chain_view,
                    &state,
                    &block_info,
                    &raw_l2tx,
                    execute_l2tx_max_cycles,
                )?
            }
            None => {
                let state = db.mem_pool_state_tree()?;
                generator.unchecked_execute_transaction(
                    &chain_view,
                    &state,
                    &block_info,
                    &raw_l2tx,
                    execute_l2tx_max_cycles,
                )?
            }
        };
        Result::<_, anyhow::Error>::Ok(run_result)
    })
    .await?;

    if run_result.exit_code != 0 {
        let receipt = gw_types::offchain::ErrorTxReceipt {
            tx_hash,
            block_number,
            return_data: run_result.return_data,
            last_log: run_result.logs.pop(),
        };

        return Err(RpcError::Full {
            code: INVALID_REQUEST,
            message: TransactionError::InvalidExitCode(run_result.exit_code).to_string(),
            data: Some(Box::new(ErrorTxReceipt::from(receipt))),
        });
    }

    Ok(run_result.into())
}

async fn submit_l2transaction(
    Params((l2tx,)): Params<(JsonBytes,)>,
    store: Data<Store>,
    submit_tx: Data<smol::channel::Sender<Request>>,
    rate_limiter: Data<Option<SendTransactionRateLimiter>>,
    rate_limit_config: Data<Option<RPCRateLimit>>,
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
        let db = store.begin_transaction();
        let tree = db.mem_pool_state_tree()?;
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

async fn submit_withdrawal_request(
    Params((withdrawal_request,)): Params<(JsonBytes,)>,
    generator: Data<Generator>,
    store: Data<Store>,
    submit_tx: Data<smol::channel::Sender<Request>>,
    rpc_client: Data<RPCClient>,
) -> Result<(), RpcError> {
    let withdrawal_bytes = withdrawal_request.into_bytes();
    let withdrawal = packed::WithdrawalRequest::from_slice(&withdrawal_bytes)?;

    // verify finalized custodian
    {
        let finalized_custodians = {
            let tip = store.get_tip_block()?;
            let db = store.begin_transaction();
            // query withdrawals from ckb-indexer
            let last_finalized_block_number = generator
                .rollup_context()
                .last_finalized_block_number(tip.raw().number().unpack());
            gw_mem_pool::custodian::query_finalized_custodians(
                &rpc_client,
                &db,
                vec![withdrawal.clone()].into_iter(),
                generator.rollup_context(),
                last_finalized_block_number,
            )
            .await?
            .expect_any()
        };
        let available_custodians = AvailableCustodians::from(&finalized_custodians);
        let withdrawal_generator = gw_mem_pool::withdrawal::Generator::new(
            generator.rollup_context(),
            available_custodians,
        );
        if let Err(err) = withdrawal_generator.verify_remained_amount(&withdrawal) {
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

    if let Err(err) = submit_tx.try_send(Request::Withdrawal(withdrawal)) {
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

    Ok(())
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
) -> Result<Uint128, RpcError> {
    let (short_address, sudt_id, block_number) = match params {
        GetBalanceParams::Tip(p) => (p.0, p.1, None),
        GetBalanceParams::Number(p) => p,
    };

    let db = store.begin_transaction();
    let balance = match block_number {
        Some(block_number) => {
            let tree = db.state_tree(StateContext::ReadOnlyHistory(block_number.into()))?;
            tree.get_sudt_balance(sudt_id.into(), short_address.as_bytes())?
        }
        None => {
            let tree = db.mem_pool_state_tree()?;
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
) -> Result<JsonH256, RpcError> {
    let (account_id, key, block_number) = match params {
        GetStorageAtParams::Tip(p) => (p.0, p.1, None),
        GetStorageAtParams::Number(p) => p,
    };

    let db = store.begin_transaction();
    let value = match block_number {
        Some(block_number) => {
            let tree = db.state_tree(StateContext::ReadOnlyHistory(block_number.into()))?;
            let key: H256 = to_h256(key);
            tree.get_value(account_id.into(), &key)?
        }
        None => {
            let tree = db.mem_pool_state_tree()?;
            let key: H256 = to_h256(key);
            tree.get_value(account_id.into(), &key)?
        }
    };

    let json_value = to_jsonh256(value);
    Ok(json_value)
}

async fn get_account_id_by_script_hash(
    Params((script_hash,)): Params<(JsonH256,)>,
    store: Data<Store>,
) -> Result<Option<AccountID>, RpcError> {
    let db = store.begin_transaction();
    let tree = db.mem_pool_state_tree()?;

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
) -> Result<Uint32, RpcError> {
    let (account_id, block_number) = match params {
        GetNonceParams::Tip(p) => (p.0, None),
        GetNonceParams::Number(p) => p,
    };

    let db = store.begin_transaction();
    let nonce = match block_number {
        Some(block_number) => {
            let tree = db.state_tree(StateContext::ReadOnlyHistory(block_number.into()))?;
            tree.get_nonce(account_id.into())?
        }
        None => {
            let tree = db.mem_pool_state_tree()?;
            tree.get_nonce(account_id.into())?
        }
    };

    Ok(nonce.into())
}

async fn get_script(
    Params((script_hash,)): Params<(JsonH256,)>,
    store: Data<Store>,
) -> Result<Option<Script>, RpcError> {
    let db = store.begin_transaction();
    let tree = db.mem_pool_state_tree()?;

    let script_hash = to_h256(script_hash);
    let script_opt = tree.get_script(&script_hash).map(Into::into);

    Ok(script_opt)
}

async fn get_script_hash(
    Params((account_id,)): Params<(AccountID,)>,
    store: Data<Store>,
) -> Result<JsonH256, RpcError> {
    let db = store.begin_transaction();
    let tree = db.mem_pool_state_tree()?;

    let script_hash = tree.get_script_hash(account_id.into())?;
    Ok(to_jsonh256(script_hash))
}

async fn get_script_hash_by_short_address(
    Params((short_address,)): Params<(JsonBytes,)>,
    store: Data<Store>,
) -> Result<Option<JsonH256>, RpcError> {
    let db = store.begin_transaction();
    let tree = db.mem_pool_state_tree()?;
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
    store: Data<Store>,
) -> Result<Option<JsonBytes>, RpcError> {
    let (data_hash, _block_number) = match params {
        GetDataParams::Tip(p) => (p.0, None),
        GetDataParams::Number(p) => p,
    };

    let db = store.begin_transaction();
    let tree = db.mem_pool_state_tree()?;

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
    *PROFILER_GUARD.lock().unwrap() = Some(ProfilerGuard::new(100).unwrap());
    Ok(())
}

async fn report_pprof() -> Result<()> {
    if let Some(profiler) = PROFILER_GUARD.lock().unwrap().take() {
        smol::spawn(async move {
            if let Ok(report) = profiler.report().build() {
                let file = std::fs::File::create("/code/workspace/flamegraph.svg").unwrap();
                let mut options = pprof::flamegraph::Options::default();
                options.image_width = Some(2500);
                report.flamegraph_with_options(file, &mut options).unwrap();
            }
        })
        .detach()
    }
    Ok(())
}

// async fn dump_mem_block(mem_pool_batch: Data<Option<MemPoolBatch>>) -> Result<JsonBytes, RpcError> {
//     let mem_pool_batch = match &*mem_pool_batch {
//         Some(mem_pool_batch) => mem_pool_batch,
//         None => {
//             return Err(mem_pool_is_disabled_err());
//         }
//     };

//     let mem_block = mem_pool_batch.dump_mem_block()?.await?;

//     Ok(JsonBytes::from_bytes(mem_block.as_bytes()))
// }

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
