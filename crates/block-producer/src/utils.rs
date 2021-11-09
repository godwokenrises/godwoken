use crate::debugger;
use anyhow::{anyhow, Result};
use async_jsonrpc_client::Output;
use async_jsonrpc_client::{Params as ClientParams, Transport};
use ckb_fixed_hash::H256;
use gw_chain::chain::{
    Chain, ChallengeCell, L1Action, L1ActionContext, RevertL1ActionContext, RevertedAction,
    RevertedL1Action, SyncParam, UpdateAction,
};
use gw_config::DebugConfig;
use gw_jsonrpc_types::ckb_jsonrpc_types::TransactionWithStatus;
use gw_jsonrpc_types::ckb_jsonrpc_types::{BlockNumber, HeaderView, Uint32};
use gw_rpc_client::indexer_types::{Order, Pagination, ScriptType, SearchKey, SearchKeyFilter, Tx};
use gw_rpc_client::rpc_client::RPCClient;
use gw_types::packed::{CellOutput, DepositRequest, Script, Transaction};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    offchain::{global_state_from_slice, RollupContext, TxStatus},
    packed::{
        CellInput, ChallengeLockArgs, ChallengeLockArgsReader, DepositLockArgs,
        L2BlockCommittedInfo, OutPoint, RollupAction, RollupActionUnion, WitnessArgs,
        WitnessArgsReader,
    },
    prelude::*,
};
use gw_web3_indexer::indexer::Web3Indexer;
use serde::de::DeserializeOwned;
use serde_json::from_value;
use serde_json::json;
use smol::lock::Mutex;
use std::path::Path;
use std::{collections::HashSet, sync::Arc};

// convert json output to result
pub fn to_result<T: DeserializeOwned>(output: Output) -> Result<T> {
    match output {
        Output::Success(success) => Ok(from_value(success.result)?),
        Output::Failure(failure) => Err(anyhow!("JSONRPC error: {}", failure.error)),
    }
}

pub async fn dry_run_transaction(
    debug_config: &DebugConfig,
    rpc_client: &RPCClient,
    tx: Transaction,
    action: &str,
) -> Option<u64> {
    if debug_config.output_l1_tx_cycles {
        let dry_run_result = rpc_client.dry_run_transaction(tx.clone()).await;
        match dry_run_result {
            Ok(cycles) => {
                log::info!(
                    "Tx({}) {} execution cycles: {}",
                    action,
                    hex::encode(tx.hash()),
                    cycles
                );
                return Some(cycles);
            }
            Err(err) => log::error!(
                "Fail to dry run transaction {}, error: {}",
                hex::encode(tx.hash()),
                err
            ),
        }
    }
    None
}

pub async fn dump_transaction<P: AsRef<Path>>(dir: P, rpc_client: &RPCClient, tx: Transaction) {
    if let Err(err) = debugger::dump_transaction(dir, rpc_client, tx.clone()).await {
        log::error!(
            "Faild to dump transaction {} error: {}",
            hex::encode(&tx.hash()),
            err
        );
    }
}

pub async fn extract_deposit_requests(
    rpc_client: &RPCClient,
    rollup_context: &RollupContext,
    tx: &Transaction,
) -> Result<(Vec<DepositRequest>, HashSet<Script>)> {
    let mut results = vec![];
    let mut asset_type_scripts = HashSet::new();
    for input in tx.raw().inputs().into_iter() {
        // Load cell denoted by the transaction input
        let tx_hash: H256 = input.previous_output().tx_hash().unpack();
        let index = input.previous_output().index().unpack();
        let tx: Option<TransactionWithStatus> = to_result(
            rpc_client
                .ckb
                .request(
                    "get_transaction",
                    Some(ClientParams::Array(vec![json!(tx_hash)])),
                )
                .await?,
        )?;
        let tx_with_status =
            tx.ok_or_else(|| anyhow::anyhow!("Cannot locate transaction: {:x}", tx_hash))?;
        let tx = {
            let tx: ckb_types::packed::Transaction = tx_with_status.transaction.inner.into();
            Transaction::new_unchecked(tx.as_bytes())
        };
        let cell_output = tx
            .raw()
            .outputs()
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("OutPoint index out of bound"))?;
        let cell_data = tx
            .raw()
            .outputs_data()
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("OutPoint index out of bound"))?;

        // Check if loaded cell is a deposit request
        if let Some(deposit_request) =
            try_parse_deposit_request(&cell_output, &cell_data.unpack(), &rollup_context)
        {
            results.push(deposit_request);
            if let Some(type_) = &cell_output.type_().to_opt() {
                asset_type_scripts.insert(type_.clone());
            }
        }
    }
    Ok((results, asset_type_scripts))
}

fn try_parse_deposit_request(
    cell_output: &CellOutput,
    cell_data: &Bytes,
    rollup_context: &RollupContext,
) -> Option<DepositRequest> {
    if cell_output.lock().code_hash() != rollup_context.rollup_config.deposit_script_type_hash()
        || cell_output.lock().hash_type() != ScriptHashType::Type.into()
    {
        return None;
    }
    let args = cell_output.lock().args().raw_data();
    if args.len() < 32 {
        return None;
    }
    let rollup_type_script_hash: [u8; 32] = rollup_context.rollup_script_hash.into();
    if args.slice(0..32) != rollup_type_script_hash[..] {
        return None;
    }
    let lock_args = match DepositLockArgs::from_slice(&args.slice(32..)) {
        Ok(lock_args) => lock_args,
        Err(_) => return None,
    };
    // NOTE: In readoly mode, we are only loading on chain data here, timeout validation
    // can be skipped. For generator part, timeout validation needs to be introduced.
    let (amount, sudt_script_hash) = match cell_output.type_().to_opt() {
        Some(script) => {
            if cell_data.len() < 16 {
                return None;
            }
            let mut data = [0u8; 16];
            data.copy_from_slice(&cell_data[0..16]);
            (u128::from_le_bytes(data), script.hash())
        }
        None => (0u128, [0u8; 32]),
    };
    let capacity: u64 = cell_output.capacity().unpack();
    let deposit_request = DepositRequest::new_builder()
        .capacity(capacity.pack())
        .amount(amount.pack())
        .sudt_script_hash(sudt_script_hash.pack())
        .script(lock_args.layer2_lock())
        .build();
    Some(deposit_request)
}
