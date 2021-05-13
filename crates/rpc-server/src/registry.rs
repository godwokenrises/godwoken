use anyhow::Result;
use ckb_types::prelude::{Builder, Entity};
use gw_common::{state::State, H256};
use gw_jsonrpc_types::{
    blockchain::Script,
    ckb_jsonrpc_types::{JsonBytes, Uint128, Uint32},
    godwoken::{L2BlockView, RunResult, TxReceipt},
};
use gw_store::{
    state_db::{StateDBTransaction, StateDBVersion},
    Store,
};
use gw_traits::CodeStore;
use gw_types::{
    packed::{self, BlockInfo},
    prelude::*,
};
use jsonrpc_v2::{Data, MapRouter, Params, Server, Server as JsonrpcServer};
use parking_lot::Mutex;
use std::sync::Arc;

// type alias
type RPCServer = Arc<Server<MapRouter>>;
type MemPool = Mutex<gw_mem_pool::pool::MemPool>;
type AccountID = Uint32;
type JsonH256 = ckb_fixed_hash::H256;

fn to_h256(v: JsonH256) -> H256 {
    let h: [u8; 32] = v.into();
    h.into()
}

fn to_jsonh256(v: H256) -> JsonH256 {
    let h: [u8; 32] = v.into();
    h.into()
}

pub struct Registry {
    mem_pool: Arc<MemPool>,
    store: Store,
}

impl Registry {
    pub fn new(mem_pool: Arc<MemPool>, store: Store) -> Self {
        Self { mem_pool, store }
    }

    pub fn build_rpc_server(self) -> Result<RPCServer> {
        let mut server = JsonrpcServer::new();

        server = server
            .with_data(Data(self.mem_pool.clone()))
            .with_data(Data::new(self.store))
            .with_method("ping", ping)
            .with_method("get_tip_block_hash", get_tip_block_hash)
            .with_method("get_block_hash", get_block_hash)
            .with_method("get_block", get_block)
            .with_method("get_block_by_number", get_block_by_number)
            .with_method("get_balance", get_balance)
            .with_method("get_storage_at", get_storage_at)
            .with_method(
                "get_account_id_by_script_hash",
                get_account_id_by_script_hash,
            )
            .with_method("get_nonce", get_nonce)
            .with_method("get_script", get_script)
            .with_method("get_script_hash", get_script_hash)
            .with_method("get_data", get_data)
            .with_method("get_transaction_receipt", get_transaction_receipt)
            .with_method("execute_l2transaction", execute_l2transaction)
            .with_method("execute_raw_l2transaction", execute_raw_l2transaction)
            .with_method("submit_l2transaction", submit_l2transaction)
            .with_method("submit_withdrawal_request", submit_withdrawal_request);

        Ok(server.finish())
    }
}

async fn ping() -> Result<String> {
    Ok("pong".to_string())
}

async fn get_block(
    Params((block_hash,)): Params<(JsonH256,)>,
    store: Data<Store>,
) -> Result<Option<L2BlockView>> {
    let block_hash = to_h256(block_hash);
    let db = store.begin_transaction();
    let block_opt = db.get_block(&block_hash)?.map(|block| {
        let block_view: L2BlockView = block.into();
        block_view
    });
    Ok(block_opt)
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
    let receipt_opt = db.get_transaction_receipt(&tx_hash)?.map(|receipt| {
        let receipt: TxReceipt = receipt.into();
        receipt
    });
    Ok(receipt_opt)
}

async fn execute_l2transaction(
    Params((l2tx,)): Params<(JsonBytes,)>,
    mem_pool: Data<MemPool>,
    store: Data<Store>,
) -> Result<RunResult> {
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

    let run_result: RunResult = mem_pool.lock().execute_transaction(tx, &block_info)?.into();
    Ok(run_result)
}

async fn execute_raw_l2transaction(
    Params((raw_l2tx,)): Params<(JsonBytes,)>,
    mem_pool: Data<MemPool>,
    store: Data<Store>,
) -> Result<RunResult> {
    let raw_l2tx_bytes = raw_l2tx.into_bytes();
    let raw_l2tx = packed::RawL2Transaction::from_slice(&raw_l2tx_bytes)?;

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
        .execute_raw_transaction(raw_l2tx, &block_info)?
        .into();
    Ok(run_result)
}

async fn submit_l2transaction(
    Params((l2tx,)): Params<(JsonBytes,)>,
    mem_pool: Data<MemPool>,
) -> Result<JsonH256> {
    let l2tx_bytes = l2tx.into_bytes();
    let tx = packed::L2Transaction::from_slice(&l2tx_bytes)?;
    let tx_hash = to_jsonh256(tx.hash().into());
    mem_pool.lock().push_transaction(tx)?;
    Ok(tx_hash)
}

async fn submit_withdrawal_request(
    Params((withdrawal_request,)): Params<(JsonBytes,)>,
    mem_pool: Data<MemPool>,
) -> Result<()> {
    let withdrawal_bytes = withdrawal_request.into_bytes();
    let withdrawal = packed::WithdrawalRequest::from_slice(&withdrawal_bytes)?;

    mem_pool.lock().push_withdrawal_request(withdrawal)?;
    Ok(())
}

async fn get_balance(
    Params((account_id, sudt_id, block_number)): Params<(
        AccountID,
        AccountID,
        gw_jsonrpc_types::ckb_jsonrpc_types::Uint64,
    )>,
    store: Data<Store>,
) -> Result<Option<Uint128>> {
    let block_number = block_number.value();
    let db = store.begin_transaction();
    let block_hash = match db.get_block_hash_by_number(block_number)? {
        Some(block_hash) => block_hash,
        None => return Ok(None),
    };
    let state_db = StateDBTransaction::from_version(
        &db,
        StateDBVersion::from_history_state(&db, block_hash, None)?,
    )?;

    let tree = state_db.account_state_tree()?;
    let balance = tree.get_sudt_balance(sudt_id.into(), account_id.into())?;

    Ok(Some(balance.into()))
}

async fn get_storage_at(
    Params((account_id, key, block_number)): Params<(
        AccountID,
        JsonH256,
        gw_jsonrpc_types::ckb_jsonrpc_types::Uint64,
    )>,
    store: Data<Store>,
) -> Result<Option<JsonH256>> {
    let block_number = block_number.value();
    let db = store.begin_transaction();
    let block_hash = match db.get_block_hash_by_number(block_number)? {
        Some(block_hash) => block_hash,
        None => return Ok(None),
    };
    let state_db = StateDBTransaction::from_version(
        &db,
        StateDBVersion::from_history_state(&db, block_hash, None)?,
    )?;

    let tree = state_db.account_state_tree()?;
    let key: H256 = to_h256(key);
    let value = tree.get_value(account_id.into(), &key)?;

    let json_value = to_jsonh256(value);
    Ok(Some(json_value))
}

async fn get_account_id_by_script_hash(
    Params((script_hash,)): Params<(JsonH256,)>,
    store: Data<Store>,
) -> Result<Option<AccountID>> {
    let db = store.begin_transaction();
    let tip_hash = db.get_tip_block_hash()?;
    let state_db = StateDBTransaction::from_version(
        &db,
        StateDBVersion::from_history_state(&db, tip_hash, None)?,
    )?;
    let tree = state_db.account_state_tree()?;

    let script_hash = to_h256(script_hash);

    let account_id_opt = tree
        .get_account_id_by_script_hash(&script_hash)?
        .map(Into::into);

    Ok(account_id_opt)
}

async fn get_nonce(
    Params((account_id, block_number)): Params<(
        AccountID,
        gw_jsonrpc_types::ckb_jsonrpc_types::Uint64,
    )>,
    store: Data<Store>,
) -> Result<Option<Uint32>> {
    let block_number = block_number.value();
    let db = store.begin_transaction();
    let block_hash = match db.get_block_hash_by_number(block_number)? {
        Some(block_hash) => block_hash,
        None => return Ok(None),
    };
    let state_db = StateDBTransaction::from_version(
        &db,
        StateDBVersion::from_history_state(&db, block_hash, None)?,
    )?;
    let tree = state_db.account_state_tree()?;

    let nonce = tree.get_nonce(account_id.into())?;

    Ok(Some(nonce.into()))
}

async fn get_script(
    Params((script_hash,)): Params<(JsonH256,)>,
    store: Data<Store>,
) -> Result<Option<Script>> {
    let db = store.begin_transaction();
    let tip_hash = db.get_tip_block_hash()?;
    let state_db = StateDBTransaction::from_version(
        &db,
        StateDBVersion::from_history_state(&db, tip_hash, None)?,
    )?;
    let tree = state_db.account_state_tree()?;

    let script_hash = to_h256(script_hash);
    let script_opt = tree.get_script(&script_hash).map(Into::into);

    Ok(script_opt)
}

async fn get_script_hash(
    Params((account_id,)): Params<(AccountID,)>,
    store: Data<Store>,
) -> Result<JsonH256> {
    let db = store.begin_transaction();
    let tip_hash = db.get_tip_block_hash()?;
    let state_db = StateDBTransaction::from_version(
        &db,
        StateDBVersion::from_history_state(&db, tip_hash, None)?,
    )?;
    let tree = state_db.account_state_tree()?;

    let script_hash = tree.get_script_hash(account_id.into())?;
    Ok(to_jsonh256(script_hash))
}

async fn get_data(
    Params((data_hash,)): Params<(JsonH256,)>,
    store: Data<Store>,
) -> Result<Option<JsonBytes>> {
    let db = store.begin_transaction();
    let tip_hash = db.get_tip_block_hash()?;
    let state_db = StateDBTransaction::from_version(
        &db,
        StateDBVersion::from_history_state(&db, tip_hash, None)?,
    )?;
    let tree = state_db.account_state_tree()?;

    let data_opt = tree
        .get_data(&to_h256(data_hash))
        .map(JsonBytes::from_bytes);

    Ok(data_opt)
}
