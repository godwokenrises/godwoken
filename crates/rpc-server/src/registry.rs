use anyhow::{anyhow, Result};
use async_trait::async_trait;
use ckb_types::prelude::{Builder, Entity};
use gw_chain::chain::Chain;
use gw_common::{state::State, H256};
use gw_config::DebugConfig;
use gw_generator::{sudt::build_l2_sudt_script, Generator};
use gw_jsonrpc_types::{
    blockchain::Script,
    ckb_jsonrpc_types::{JsonBytes, Uint128, Uint32},
    debugger::{DumpChallengeTarget, ReprMockTransaction},
    godwoken::{
        GlobalState, L2BlockStatus, L2BlockView, L2BlockWithStatus, L2TransactionStatus,
        L2TransactionWithStatus, RunResult, TxReceipt,
    },
    test_mode::{ShouldProduceBlock, TestModePayload},
};
use gw_store::{
    state_db::{CheckPoint, StateDBMode, StateDBTransaction, SubState},
    transaction::StoreTransaction,
    Store,
};
use gw_traits::CodeStore;
use gw_types::{
    packed::{self, BlockInfo, RawL2Block, RollupConfig},
    prelude::*,
};
use jsonrpc_v2::{Data, Error as RpcError, MapRouter, Params, Server, Server as JsonrpcServer};
use smol::lock::Mutex;
use std::sync::Arc;

// type alias
type RPCServer = Arc<Server<MapRouter>>;
type MemPool = Option<Arc<Mutex<gw_mem_pool::pool::MemPool>>>;
type AccountID = Uint32;
type JsonH256 = ckb_fixed_hash::H256;
type BoxedTestsRPCImpl = Box<dyn TestModeRPC + Send + Sync>;
type GwUint64 = gw_jsonrpc_types::ckb_jsonrpc_types::Uint64;

const HEADER_NOT_FOUND_ERR_CODE: i64 = -32000;
const METHOD_NOT_AVAILABLE_ERR_CODE: i64 = -32601;

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

#[allow(clippy::needless_lifetimes)]
async fn get_state_db_at_block<'a>(
    db: &'a StoreTransaction,
    mem_pool: &MemPool,
    block_number: Option<gw_jsonrpc_types::ckb_jsonrpc_types::Uint64>,
) -> Result<StateDBTransaction<'a>, RpcError> {
    let tip_block_number = db.get_tip_block()?.raw().number().unpack();
    match block_number.map(|n| n.value()) {
        Some(block_number) => {
            if block_number > tip_block_number {
                return Err(header_not_found_err());
            }
            StateDBTransaction::from_checkpoint(
                &db,
                CheckPoint::new(block_number, SubState::Block),
                StateDBMode::ReadOnly,
            )
            .map_err(Into::into)
        }
        None => match mem_pool {
            Some(mem_pool) => {
                let mem_pool = mem_pool.lock().await;
                mem_pool.fetch_state_db(&db).map_err(Into::into)
            }
            None => {
                // fallback to tip number
                StateDBTransaction::from_checkpoint(
                    &db,
                    CheckPoint::new(tip_block_number, SubState::Block),
                    StateDBMode::ReadOnly,
                )
                .map_err(Into::into)
            }
        },
    }
}

pub struct Registry {
    generator: Arc<Generator>,
    mem_pool: MemPool,
    store: Store,
    tests_rpc_impl: Option<Arc<BoxedTestsRPCImpl>>,
    rollup_config: RollupConfig,
    debug_config: DebugConfig,
    chain: Arc<Mutex<Chain>>,
}

impl Registry {
    pub fn new<T>(
        store: Store,
        mem_pool: MemPool,
        generator: Arc<Generator>,
        tests_rpc_impl: Option<Box<T>>,
        rollup_config: RollupConfig,
        debug_config: DebugConfig,
        chain: Arc<Mutex<Chain>>,
    ) -> Self
    where
        T: TestModeRPC + Send + Sync + 'static,
    {
        Self {
            mem_pool,
            store,
            generator,
            tests_rpc_impl: tests_rpc_impl
                .map(|r| Arc::new(r as Box<dyn TestModeRPC + Sync + Send + 'static>)),
            rollup_config,
            debug_config,
            chain,
        }
    }

    pub fn build_rpc_server(self) -> Result<RPCServer> {
        let mut server = JsonrpcServer::new();

        server = server
            .with_data(Data::new(self.mem_pool))
            .with_data(Data(self.generator.clone()))
            .with_data(Data::new(self.store))
            .with_data(Data::new(self.rollup_config))
            .with_method("gw_ping", ping)
            .with_method("gw_get_tip_block_hash", get_tip_block_hash)
            .with_method("gw_get_block_hash", get_block_hash)
            .with_method("gw_get_block", get_block)
            .with_method("gw_get_block_by_number", get_block_by_number)
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
            .with_method("gw_submit_l2transaction", submit_l2transaction)
            .with_method("gw_submit_withdrawal_request", submit_withdrawal_request)
            .with_method(
                "gw_compute_l2_sudt_script_hash",
                compute_l2_sudt_script_hash,
            );

        // Tests
        if let Some(tests_rpc_impl) = self.tests_rpc_impl {
            server = server
                .with_data(Data(Arc::clone(&tests_rpc_impl)))
                .with_method("tests_produce_block", tests_produce_block)
                .with_method("tests_should_produce_block", tests_should_produce_block)
                .with_method("tests_get_global_state", tests_get_global_state);
        }

        // Debug
        if self.debug_config.enable_debug_rpc {
            server = server.with_data(Data::new(self.chain)).with_method(
                "debug_dump_cancel_challenge_tx",
                debug_dump_cancel_challenge_tx,
            );
        }

        Ok(server.finish())
    }
}

async fn ping() -> Result<String> {
    Ok("pong".to_string())
}

async fn get_transaction(
    Params((tx_hash,)): Params<(JsonH256,)>,
    mem_pool: Data<MemPool>,
    store: Data<Store>,
) -> Result<Option<L2TransactionWithStatus>> {
    let tx_hash = to_h256(tx_hash);
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
        None => match &*mem_pool {
            Some(mem_pool) => {
                let mem_pool = mem_pool.lock().await;
                tx_opt = mem_pool.all_txs().get(&tx_hash).cloned();
                status = L2TransactionStatus::Pending;
            }
            None => {
                tx_opt = None;
                status = L2TransactionStatus::Pending;
            }
        },
    };

    Ok(tx_opt.map(|tx| L2TransactionWithStatus {
        transaction: tx.into(),
        status,
    }))
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
    mem_pool: Data<MemPool>,
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
    let receipt_opt = match mem_pool.as_deref() {
        Some(mem_pool) => mem_pool
            .lock()
            .await
            .mem_block()
            .tx_receipts()
            .get(&tx_hash)
            .map(ToOwned::to_owned)
            .map(Into::into),
        None => None,
    };

    Ok(receipt_opt)
}

async fn execute_l2transaction(
    Params((l2tx,)): Params<(JsonBytes,)>,
    mem_pool: Data<MemPool>,
    store: Data<Store>,
) -> Result<RunResult, RpcError> {
    let mem_pool = match &*mem_pool {
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

    let run_result: RunResult = mem_pool
        .lock()
        .await
        .execute_transaction(tx, &block_info)?
        .into();
    Ok(run_result)
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
    mem_pool: Data<MemPool>,
    store: Data<Store>,
) -> Result<RunResult, RpcError> {
    let mem_pool = match mem_pool.clone() {
        Some(mem_pool) => mem_pool,
        None => {
            return Err(mem_pool_is_disabled_err());
        }
    };
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
        None => mem_pool.lock().await.mem_block().block_info().to_owned(),
    };

    // execute tx in task
    let run_result = smol::spawn(async move {
        mem_pool
            .lock()
            .await
            .execute_raw_transaction(raw_l2tx, &block_info, block_number_opt)
    })
    .await?
    .into();
    Ok(run_result)
}

async fn submit_l2transaction(
    Params((l2tx,)): Params<(JsonBytes,)>,
    mem_pool: Data<MemPool>,
) -> Result<JsonH256, RpcError> {
    let mem_pool = match mem_pool.clone() {
        Some(mem_pool) => mem_pool,
        None => {
            return Err(mem_pool_is_disabled_err());
        }
    };
    let l2tx_bytes = l2tx.into_bytes();
    let tx = packed::L2Transaction::from_slice(&l2tx_bytes)?;
    let tx_hash = to_jsonh256(tx.hash().into());
    // run task in the background
    smol::spawn(async move {
        if let Err(err) = mem_pool.lock().await.push_transaction(tx.clone()) {
            log::info!(
                "[RPC] fail to push tx {:?} into mem-pool, err: {}",
                faster_hex::hex_string(&tx.hash()),
                err
            );
        }
    })
    .detach();
    Ok(tx_hash)
}

async fn submit_withdrawal_request(
    Params((withdrawal_request,)): Params<(JsonBytes,)>,
    mem_pool: Data<MemPool>,
) -> Result<(), RpcError> {
    let mem_pool = match &*mem_pool {
        Some(mem_pool) => mem_pool,
        None => {
            return Err(mem_pool_is_disabled_err());
        }
    };
    let withdrawal_bytes = withdrawal_request.into_bytes();
    let withdrawal = packed::WithdrawalRequest::from_slice(&withdrawal_bytes)?;

    mem_pool.lock().await.push_withdrawal_request(withdrawal)?;
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
    mem_pool: Data<MemPool>,
    store: Data<Store>,
) -> Result<Uint128, RpcError> {
    let (short_address, sudt_id, block_number) = match params {
        GetBalanceParams::Tip(p) => (p.0, p.1, None),
        GetBalanceParams::Number(p) => p,
    };

    let db = store.begin_transaction();
    let state_db = get_state_db_at_block(&db, &mem_pool, block_number).await?;
    let tree = state_db.state_tree()?;
    let balance = tree.get_sudt_balance(sudt_id.into(), short_address.as_bytes())?;
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
    mem_pool: Data<MemPool>,
    store: Data<Store>,
) -> Result<JsonH256, RpcError> {
    let (account_id, key, block_number) = match params {
        GetStorageAtParams::Tip(p) => (p.0, p.1, None),
        GetStorageAtParams::Number(p) => p,
    };

    let db = store.begin_transaction();
    let state_db = get_state_db_at_block(&db, &mem_pool, block_number).await?;

    let tree = state_db.state_tree()?;
    let key: H256 = to_h256(key);
    let value = tree.get_value(account_id.into(), &key)?;

    let json_value = to_jsonh256(value);
    Ok(json_value)
}

async fn get_account_id_by_script_hash(
    Params((script_hash,)): Params<(JsonH256,)>,
    mem_pool: Data<MemPool>,
    store: Data<Store>,
) -> Result<Option<AccountID>, RpcError> {
    let db = store.begin_transaction();
    let state_db = get_state_db_at_block(&db, &mem_pool, None).await?;
    let tree = state_db.state_tree()?;

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
    mem_pool: Data<MemPool>,
    store: Data<Store>,
) -> Result<Uint32, RpcError> {
    let (account_id, block_number) = match params {
        GetNonceParams::Tip(p) => (p.0, None),
        GetNonceParams::Number(p) => p,
    };

    let db = store.begin_transaction();
    let state_db = get_state_db_at_block(&db, &mem_pool, block_number).await?;
    let tree = state_db.state_tree()?;

    let nonce = tree.get_nonce(account_id.into())?;

    Ok(nonce.into())
}

async fn get_script(
    Params((script_hash,)): Params<(JsonH256,)>,
    mem_pool: Data<MemPool>,
    store: Data<Store>,
) -> Result<Option<Script>, RpcError> {
    let db = store.begin_transaction();
    let state_db = get_state_db_at_block(&db, &mem_pool, None).await?;
    let tree = state_db.state_tree()?;

    let script_hash = to_h256(script_hash);
    let script_opt = tree.get_script(&script_hash).map(Into::into);

    Ok(script_opt)
}

async fn get_script_hash(
    Params((account_id,)): Params<(AccountID,)>,
    mem_pool: Data<MemPool>,
    store: Data<Store>,
) -> Result<JsonH256, RpcError> {
    let db = store.begin_transaction();
    let state_db = get_state_db_at_block(&db, &mem_pool, None).await?;
    let tree = state_db.state_tree()?;

    let script_hash = tree.get_script_hash(account_id.into())?;
    Ok(to_jsonh256(script_hash))
}

async fn get_script_hash_by_short_address(
    Params((short_address,)): Params<(JsonBytes,)>,
    mem_pool: Data<MemPool>,
    store: Data<Store>,
) -> Result<Option<JsonH256>, RpcError> {
    let db = store.begin_transaction();
    let state_db = get_state_db_at_block(&db, &mem_pool, None).await?;
    let tree = state_db.state_tree()?;
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
    mem_pool: Data<MemPool>,
    store: Data<Store>,
) -> Result<Option<JsonBytes>, RpcError> {
    let (data_hash, block_number) = match params {
        GetDataParams::Tip(p) => (p.0, None),
        GetDataParams::Number(p) => p,
    };

    let db = store.begin_transaction();
    let state_db = get_state_db_at_block(&db, &mem_pool, block_number).await?;
    let tree = state_db.state_tree()?;

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

async fn debug_dump_cancel_challenge_tx(
    Params((target,)): Params<(DumpChallengeTarget,)>,
    chain: Data<Arc<Mutex<Chain>>>,
) -> Result<ReprMockTransaction> {
    let chain = chain.lock().await;
    let (block_hash, target_index, target_type) = match target {
        DumpChallengeTarget::ByBlockHash {
            block_hash,
            target_index,
            target_type,
        } => (to_h256(block_hash), target_index, target_type),
        DumpChallengeTarget::ByBlockNumber {
            block_number,
            target_index,
            target_type,
        } => {
            let block_hash = {
                let db = chain.store().begin_transaction();
                db.get_block_hash_by_number(block_number.into())?
                    .ok_or_else(|| anyhow!("block {} hash not found", block_number))?
            };
            (block_hash, target_index, target_type)
        }
    };

    let target = gw_types::packed::ChallengeTarget::new_builder()
        .block_hash(Into::<[u8; 32]>::into(block_hash).pack())
        .target_index(target_index.value().pack())
        .target_type(target_type.into())
        .build();

    chain.dump_cancel_challenge_tx(target)
}
