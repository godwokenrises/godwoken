#![allow(clippy::mutable_key_type)]

use std::{fmt::Display, sync::Arc, time::Duration};

use anyhow::{bail, ensure, Context, Result};
use gw_chain::chain::Chain;
use gw_common::H256;
use gw_config::PscConfig;
use gw_mem_pool::pool::MemPool;
use gw_rpc_client::{
    error::{get_jsonrpc_error_code, CkbRpcError},
    rpc_client::RPCClient,
};
use gw_store::{snapshot::StoreSnapshot, traits::chain_store::ChainStore, Store};
use gw_types::{
    offchain::{CellStatus, DepositInfo, TxStatus},
    packed::{
        Confirmed, GlobalState, LocalBlock, NumberHash, OutPoint, Script, ScriptVec, Submitted,
        Transaction, WithdrawalKey,
    },
    prelude::*,
};
use gw_utils::{abort_on_drop::spawn_abort_on_drop, local_cells::LocalCellsManager, since::Since};
use tokio::{sync::Mutex, time::Instant};

use crate::{
    block_producer::{BlockProducer, ComposeSubmitTxArgs},
    block_sync_server::BlockSyncServerState,
    chain_updater::ChainUpdater,
    produce_block::ProduceBlockResult,
    sync_l1::{revert, sync_l1, SyncL1Context},
};

/// Block producing, submitting and confirming state machine.
pub struct ProduceSubmitConfirm {
    context: Arc<PSCContext>,
    local_count: u64,
    submitted_count: u64,
}

pub struct PSCContext {
    pub store: Store,
    pub rpc_client: RPCClient,
    pub chain: Arc<Mutex<Chain>>,
    pub mem_pool: Arc<Mutex<MemPool>>,
    pub block_producer: BlockProducer,
    // Use mutex to make rust happy. Actually we won't refresh or access this at
    // the same time.
    pub local_cells_manager: Mutex<LocalCellsManager>,
    pub chain_updater: ChainUpdater,
    pub rollup_type_script: Script,
    pub psc_config: PscConfig,
    pub block_sync_server_state: Option<Arc<std::sync::Mutex<BlockSyncServerState>>>,
}

impl SyncL1Context for PSCContext {
    fn store(&self) -> &Store {
        &self.store
    }
    fn rpc_client(&self) -> &RPCClient {
        &self.rpc_client
    }
    fn chain(&self) -> &Mutex<Chain> {
        &self.chain
    }
    fn mem_pool(&self) -> &Mutex<MemPool> {
        &self.mem_pool
    }
    fn chain_updater(&self) -> &ChainUpdater {
        &self.chain_updater
    }
    fn rollup_type_script(&self) -> &Script {
        &self.rollup_type_script
    }
}

impl ProduceSubmitConfirm {
    pub async fn init(context: Arc<PSCContext>) -> Result<Self> {
        sync_l1(&*context).await?;
        // Get again because they may have changed after syncing with L1.
        let snap = context.store.get_snapshot();
        let last_valid = snap.get_last_valid_tip_block()?.raw().number().unpack();
        let last_submitted = snap
            .get_last_submitted_block_number_hash()
            .expect("get last submitted")
            .number()
            .unpack();
        let last_confirmed = snap
            .get_last_confirmed_block_number_hash()
            .expect("get last confirmed")
            .number()
            .unpack();
        {
            let mut local_cells_manager = context.local_cells_manager.lock().await;
            for b in last_confirmed + 1..=last_submitted {
                let tx = snap.get_block_submit_tx(b).expect("submit tx");
                local_cells_manager.apply_tx(&tx.as_reader());
            }
            for b in last_submitted + 1..=last_valid {
                let deposits = snap.get_block_deposit_info_vec(b).expect("deposit info");
                let deposits = deposits.into_iter().map(|d| d.cell());
                for c in deposits {
                    local_cells_manager.lock_cell(c.out_point());
                }
            }
            let mut pool = context.mem_pool.lock().await;
            pool.notify_new_tip(snap.get_last_valid_tip_block_hash()?, &local_cells_manager)
                .await?;
            pool.mem_pool_state().set_completed_initial_syncing();
        }
        // Publish initial messages.
        if let Some(ref sync_server) = context.block_sync_server_state {
            let mut sync_server = sync_server.lock().unwrap();
            for b in last_confirmed + 1..=last_valid {
                publish_local_block(&mut sync_server, &snap, b)?;
            }
            for b in last_confirmed + 1..=last_submitted {
                publish_submitted(&mut sync_server, &snap, b)?;
            }
        }

        log::info!(
            "last valid: {}, last_submitted: {}, last_confirmed: {}",
            last_valid,
            last_submitted,
            last_confirmed
        );
        let local_count = last_valid - last_submitted;
        let submitted_count = last_submitted - last_confirmed;
        Ok(Self {
            context,
            local_count,
            submitted_count,
        })
    }

    /// Run the producing, submitting and confirming loop.
    pub async fn run(mut self) -> Result<()> {
        loop {
            match run(&mut self).await {
                Ok(()) => return Ok(()),
                Err(e) if is_should_revert_error(&e) => {
                    log::warn!("Error: {:#}", e);

                    let error_block = e.downcast::<BlockContext>()?.0;
                    {
                        let store_tx = self.context.store.begin_transaction();
                        log::info!("revert to block {}", error_block - 1);
                        revert(&*self.context, &store_tx, error_block - 1).await?;
                        store_tx.commit()?;
                    }

                    sync_l1(&*self.context).await?;

                    // TODO: publish block sync messages.

                    // Reset local_count, submitted_count and local_cells_manager.
                    let snap = self.context.store.get_snapshot();
                    let last_valid = snap.get_last_valid_tip_block()?.raw().number().unpack();
                    let last_submitted = snap
                        .get_last_submitted_block_number_hash()
                        .expect("get last submitted")
                        .number()
                        .unpack();
                    let last_confirmed = snap
                        .get_last_confirmed_block_number_hash()
                        .expect("get last confirmed")
                        .number()
                        .unpack();
                    {
                        let mut local_cells_manager = self.context.local_cells_manager.lock().await;
                        local_cells_manager.reset();
                        for b in last_confirmed + 1..=last_submitted {
                            let tx = snap.get_block_submit_tx(b).expect("submit tx");
                            local_cells_manager.apply_tx(&tx.as_reader());
                        }
                        for b in last_submitted + 1..=last_valid {
                            let deposits =
                                snap.get_block_deposit_info_vec(b).expect("deposit info");
                            let deposits = deposits.into_iter().map(|d| d.cell());
                            for c in deposits {
                                local_cells_manager.lock_cell(c.out_point());
                            }
                        }
                        let new_tip = snap.get_last_valid_tip_block_hash()?;
                        let mut mem_pool = self.context.mem_pool.lock().await;
                        mem_pool
                            .notify_new_tip(new_tip, &local_cells_manager)
                            .await?;
                    }
                    self.local_count = last_valid - last_submitted;
                    self.submitted_count = last_submitted - last_confirmed;
                }
                Err(e) => return Err(e),
            }
        }
    }
}

async fn run(mut state: &mut ProduceSubmitConfirm) -> Result<()> {
    let mut submitting = false;
    let mut submit_handle = spawn_abort_on_drop(async { anyhow::Ok(NumberHash::default()) });
    let mut confirming = false;
    let mut confirm_handle = spawn_abort_on_drop(async { anyhow::Ok(NumberHash::default()) });
    let config = &state.context.psc_config;
    let mut interval = tokio::time::interval(Duration::from_secs(config.block_interval_secs));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            // Produce a new local block if the produce timer has expired and
            // there are not too many local blocks.
            _ = interval.tick(), if state.local_count < config.local_limit => {
                log::info!("producing next block");
                if let Err(e) = produce_local_block(&state.context).await {
                    log::warn!("failed to produce local block: {:#}", e);
                } else {
                    state.local_count += 1;
                }
            }
            // Block submitted.
            result = &mut submit_handle, if submitting => {
                submitting = false;
                match result {
                    Err(err) if err.is_panic() => bail!("submit task panic: {:?}", err.into_panic()),
                    Ok(nh) => {
                        let nh = nh?;
                        let store_tx = state.context.store.begin_transaction();
                        store_tx.set_last_submitted_block_number_hash(&nh.as_reader())?;
                        store_tx.commit()?;
                        if let Some(ref sync_server) = state.context.block_sync_server_state {
                            let mut sync_server = sync_server.lock().unwrap();
                            publish_submitted(&mut sync_server, &state.context.store.get_snapshot(), nh.number().unpack())?;
                        }
                        state.submitted_count += 1;
                        state.local_count -= 1;
                    }
                    _ => {}
                }
            }
            // Block confirmed.
            result = &mut confirm_handle, if confirming => {
                confirming = false;
                match result {
                    Err(err) if err.is_panic() => bail!("sync task panic: {:?}", err.into_panic()),
                    Ok(nh) => {
                        let nh = nh?;
                        let store_tx = state.context.store.begin_transaction();
                        store_tx.set_last_confirmed_block_number_hash(&nh.as_reader())?;
                        store_tx.commit()?;
                        if let Some(ref sync_server) = state.context.block_sync_server_state {
                            let mut sync_server = sync_server.lock().unwrap();
                            publish_confirmed(&mut sync_server, &state.context.store.get_snapshot(), nh.number().unpack())?;
                        }
                        state.submitted_count -= 1;
                    }
                    _ => {}
                }
            }
            else => {}
        }
        if !submitting && state.local_count > 0 && state.submitted_count < config.submitted_limit {
            submitting = true;
            let context = state.context.clone();
            submit_handle.replace_with(tokio::spawn(async move {
                loop {
                    match submit_next_block(&context).await {
                        Ok(nh) => return Ok(nh),
                        Err(err) => {
                            if is_should_revert_error(&err) {
                                return Err(err);
                            }
                            log::warn!("failed to submit next block: {:#}", err);
                            // TOOO: backoff.
                            tokio::time::sleep(Duration::from_secs(20)).await;
                        }
                    }
                }
            }));
        }
        if !confirming && state.submitted_count > 0 {
            confirming = true;
            let context = state.context.clone();
            confirm_handle.replace_with(tokio::spawn(async move {
                loop {
                    match confirm_next_block(&context).await {
                        Ok(nh) => break Ok(nh),
                        Err(err) => {
                            if is_should_revert_error(&err) {
                                return Err(err);
                            }
                            log::warn!("failed to confirm next block: {:#}", err);
                            // TOOO: backoff.
                            tokio::time::sleep(Duration::from_secs(3)).await;
                        }
                    }
                }
            }));
        }
    }
}

/// Produce and save local block.
async fn produce_local_block(ctx: &PSCContext) -> Result<()> {
    // TODO: check block and retry.
    let ProduceBlockResult {
        block,
        global_state,
        withdrawal_extras,
        deposit_cells,
        remaining_capacity,
    } = ctx.block_producer.produce_next_block(0).await?;

    let number: u64 = block.raw().number().unpack();
    let block_hash: H256 = block.hash().into();

    let block_txs = block.transactions().len();
    let block_withdrawals = block.withdrawals().len();

    // Now update db about the new local L2 block

    let deposit_info_vec = deposit_cells.pack();
    let deposit_asset_scripts = deposit_cells
        .iter()
        .filter_map(|d| d.cell.output.type_().to_opt())
        .collect();

    let store_tx = ctx.store.begin_transaction();

    ctx.chain
        .lock()
        .await
        .update_local(
            &store_tx,
            block,
            deposit_info_vec,
            deposit_asset_scripts,
            withdrawal_extras,
            global_state,
        )
        .await?;

    log::info!(
        "produced new block #{} (txs: {}, deposits: {}, withdrawals: {})",
        number,
        block_txs,
        deposit_cells.len(),
        block_withdrawals,
    );

    log::info!(
        "save capacity: block: {}, capacity: {}",
        number,
        remaining_capacity.capacity
    );
    store_tx.set_block_post_finalized_custodian_capacity(
        number,
        &remaining_capacity.pack().as_reader(),
    )?;

    store_tx.commit()?;
    // Lock collected deposits and custodians.
    let mut local_cells_manager = ctx.local_cells_manager.lock().await;
    for d in deposit_cells {
        local_cells_manager.lock_cell(d.cell.out_point);
    }
    if let Some(ref sync_server_state) = ctx.block_sync_server_state {
        publish_local_block(
            &mut sync_server_state.lock().unwrap(),
            &ctx.store.get_snapshot(),
            number,
        )?;
    }
    ctx.mem_pool
        .lock()
        .await
        .notify_new_tip(block_hash, &local_cells_manager)
        .await?;

    Ok(())
}

async fn submit_next_block(ctx: &PSCContext) -> Result<NumberHash> {
    let snap = ctx.store.get_snapshot();
    // L2 block number to submit.
    let block_number = snap
        .get_last_submitted_block_number_hash()
        .expect("get last submitted block number")
        .number()
        .unpack()
        + 1;
    // L2 block hash to submit.
    let block_hash = snap
        .get_block_hash_by_number(block_number)?
        .context("failed to get next block hash")?;
    let block = snap.get_block(&block_hash)?.context("get_block")?;
    let timestamp_millis = block.raw().timestamp().unpack();
    // Godwoken scripts require that previous block timestamp < block timestamp < since:
    // https://github.com/nervosnetwork/godwoken-scripts/blob/d983fb351410eb6fbe02bb298af909193aeb5f22/contracts/state-validator/src/verifications/submit_block.rs#L707-L726
    let since = greater_since(timestamp_millis);
    let since_millis = since.extract_lock_value().unwrap().timestamp().unwrap();
    let tx = if let Some(tx) = snap.get_block_submit_tx(block_number) {
        drop(snap);
        tx
    } else {
        // Restore Vec<WithdrawalRequestExtras> from store.
        let mut withdrawal_extras = Vec::with_capacity(block.withdrawals().len());
        for (idx, w) in block.withdrawals().into_iter().enumerate() {
            let extra = snap
                .get_withdrawal_by_key(&WithdrawalKey::build_withdrawal_key(
                    block_hash.pack(),
                    idx as u32,
                ))?
                .context("get withdrawal")?;
            ensure!(extra.hash() == w.hash());
            withdrawal_extras.push(extra);
        }
        let deposit_cells: Vec<DepositInfo> = snap
            .get_block_deposit_info_vec(block_number)
            .context("get deposit info vec")?
            .unpack();
        let global_state: GlobalState = snap
            .get_block_post_global_state(&block_hash)?
            .context("get block global_state")?;
        drop(snap);

        let local_cells_manager = ctx.local_cells_manager.lock().await;

        let args = ComposeSubmitTxArgs {
            deposit_cells,
            block,
            global_state,
            since,
            withdrawal_extras,
            local_cells_manager: &*local_cells_manager,
        };
        let tx = ctx.block_producer.compose_submit_tx(args).await?;

        let store_tx = ctx.store.begin_transaction();
        store_tx.set_block_submit_tx(block_number, &tx.as_reader())?;
        store_tx.commit()?;

        log::info!(
            "generated submission transaction for block {}",
            block_number
        );

        tx
    };

    ctx.local_cells_manager
        .lock()
        .await
        .apply_tx(&tx.as_reader());

    // Wait until median >= since, or CKB will reject the transaction.
    loop {
        match median_gte(&ctx.rpc_client, since_millis).await {
            Ok(_) => break,
            Err(err) => {
                log::info!("wait for median >= {}: {:#}", since_millis, err);
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }
    }

    log::info!(
        "sending transaction 0x{} to submit block {}",
        hex::encode(tx.hash()),
        block_number
    );
    send_transaction_or_check_inputs(&ctx.rpc_client, &tx, false)
        .await
        .context(BlockContext(block_number))?;
    log::info!("tx sent");
    Ok(NumberHash::new_builder()
        .block_hash(block_hash.pack())
        .number(block_number.pack())
        .build())
}

async fn poll_tx_confirmed(rpc_client: &RPCClient, tx: &Transaction) -> Result<()> {
    log::info!("waiting for tx 0x{}", hex::encode(tx.hash()));
    let mut last_sent = Instant::now();
    loop {
        let status = rpc_client
            .ckb
            .get_transaction_status(tx.hash().into())
            .await?;
        let should_resend = match status {
            Some(TxStatus::Pending) | Some(TxStatus::Proposed) => false,
            Some(TxStatus::Committed) => break,
            Some(TxStatus::Rejected) => true,
            Some(TxStatus::Unknown) | None => last_sent.elapsed() > Duration::from_secs(20),
        };
        if should_resend {
            log::info!("resend transaction 0x{}", hex::encode(tx.hash()));
            send_transaction_or_check_inputs(rpc_client, tx, true).await?;
            last_sent = Instant::now();
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    // Wait for indexer syncing the L1 block.
    let block_number = rpc_client
        .ckb
        .get_transaction_block_number(tx.hash().into())
        .await?
        .context("get tx block hash")?;
    loop {
        let tip = rpc_client.get_tip().await?;
        if tip.number().unpack() >= block_number {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(())
}

async fn confirm_next_block(context: &PSCContext) -> Result<NumberHash> {
    let snap = context.store.get_snapshot();
    let block_number = snap
        .get_last_confirmed_block_number_hash()
        .expect("last confirmed")
        .number()
        .unpack()
        + 1;
    let block_hash = snap
        .get_block_hash_by_number(block_number)?
        .expect("block hash");
    let tx = snap
        .get_block_submit_tx(block_number)
        .expect("get submit tx");
    drop(snap);
    poll_tx_confirmed(&context.rpc_client, &tx)
        .await
        .context(BlockContext(block_number))?;
    log::info!("block {} confirmed", block_number);
    context.local_cells_manager.lock().await.confirm_tx(&tx);
    Ok(NumberHash::new_builder()
        .block_hash(block_hash.pack())
        .number(block_number.pack())
        .build())
}

/// Check that current CKB tip block median time >= timestamp.
async fn median_gte(rpc_client: &RPCClient, timestamp_millis: u64) -> Result<()> {
    let tip = rpc_client.get_tip().await?;
    let median = rpc_client
        .get_block_median_time(tip.block_hash().unpack())
        .await?;
    ensure!(median >= Some(Duration::from_millis(timestamp_millis)));
    Ok(())
}

/// Calculate a since whose timestamp > param timestamp_millis
fn greater_since(timestamp_millis: u64) -> Since {
    Since::new_timestamp_seconds(timestamp_millis / 1000 + 1)
}

#[cfg(test)]
#[test]
fn test_greater_since() {
    for t in [0, 999, 1000, 1500, 2000, u64::MAX / 1000 * 1000 - 1] {
        let since_t = greater_since(t)
            .extract_lock_value()
            .unwrap()
            .timestamp()
            .unwrap();
        assert!(since_t > t);
        assert!(since_t.saturating_sub(1000) <= t);
    }
}

async fn check_cell(rpc_client: &RPCClient, out_point: &OutPoint) -> Result<()> {
    let block_number = rpc_client
        .ckb
        .get_transaction_block_number(out_point.tx_hash().unpack())
        .await?
        .context(UnknownCellError)?;
    let mut opt_block = rpc_client.get_block_by_number(block_number).await?;
    // Search later blocks to see who consumed this cell.
    for _ in 0..100 {
        if let Some(block) = opt_block {
            for tx in block.transactions() {
                if tx
                    .raw()
                    .inputs()
                    .into_iter()
                    .any(|i| i.previous_output().eq(out_point))
                {
                    bail!(anyhow::Error::new(DeadCellError)
                        .context(format!("consumed by tx 0x{}", hex::encode(tx.hash()))))
                }
            }
            opt_block = rpc_client
                .get_block_by_number(block.header().raw().number().unpack() + 1)
                .await?;
        } else {
            return Ok(());
        }
    }
    // Transaction is on chain, but cell is not live, so it is dead.
    bail!(DeadCellError);
}

/// Send transaction.
///
/// Will check input cells if sending fails with `TransactionFailedToResolve`.
/// If any input cell is dead, the error returned will be a `DeadCellError`.
///
/// If `strict` is true, when any input cell is output of a transaction that is
/// not confirmed, the returned error will be a `UnknownCellError`.
///
/// If `strict` is false, having input cells that are output of unconfirmed
/// transactions is not an error.
async fn send_transaction_or_check_inputs(
    rpc_client: &RPCClient,
    tx: &Transaction,
    strict: bool,
) -> anyhow::Result<()> {
    if let Err(mut err) = rpc_client.send_transaction(tx).await {
        let code = get_jsonrpc_error_code(&err);
        if code == Some(CkbRpcError::TransactionFailedToResolve as i64) {
            if let Err(e) = check_tx_input(rpc_client, tx).await {
                if !strict && e.is::<UnknownCellError>() {
                    // This way err is not `UnknownCellError`.
                    err = err.context(e);
                } else {
                    // If e is some specific error, err is too.
                    err = e.context(err);
                }
            }
            Err(err)
        } else if code == Some(CkbRpcError::PoolRejectedDuplicatedTransaction as i64) {
            Ok(())
        } else {
            Err(err)
        }
    } else {
        Ok(())
    }
}

#[derive(Debug)]
struct BlockContext(u64);

impl Display for BlockContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "block {}", self.0)
    }
}

fn is_should_revert_error(e: &anyhow::Error) -> bool {
    e.is::<DeadCellError>() || e.is::<UnknownCellError>()
}

#[derive(thiserror::Error, Debug)]
#[error("dead cell")]
struct DeadCellError;

#[derive(thiserror::Error, Debug)]
#[error("previous transaction not confirmed")]
struct UnknownCellError;

async fn check_tx_input(rpc_client: &RPCClient, tx: &Transaction) -> Result<()> {
    // Check inputs.
    for input in tx.raw().inputs() {
        let out_point = input.previous_output();
        let status = rpc_client
            .get_cell(out_point.clone())
            .await?
            .map(|c| c.status);
        match status {
            Some(CellStatus::Live) => {}
            _ => {
                check_cell(rpc_client, &out_point)
                    .await
                    .with_context(|| format!("checking out point {}", &out_point))?;
            }
        }
    }
    Ok(())
}

fn publish_local_block(
    sync_server: &mut BlockSyncServerState,
    snap: &StoreSnapshot,
    b: u64,
) -> Result<()> {
    let block_hash = snap
        .get_block_hash_by_number(b)?
        .context("get block hash")?;
    let block = snap.get_block(&block_hash)?.context("get block")?;
    let global_state = snap
        .get_block_post_global_state(&block_hash)?
        .context("get block post global state")?;
    let deposit_info_vec = snap
        .get_block_deposit_info_vec(b)
        .context("get block deposit info vec")?;
    let deposit_asset_scripts = {
        let reader = deposit_info_vec.as_reader();
        let asset_hashes = reader.iter().filter_map(|r| {
            let h: H256 = r.request().sudt_script_hash().unpack();
            if h.is_zero() {
                None
            } else {
                Some(h)
            }
        });
        let asset_scripts = asset_hashes.map(|h| {
            snap.get_asset_script(&h)?
                .with_context(|| format!("block {} asset script {} not found", b, h.pack()))
        });
        asset_scripts.collect::<Result<Vec<_>>>()?
    };
    let withdrawals = {
        let reqs = block.as_reader().withdrawals();
        let extra_reqs = reqs.iter().map(|w| {
            let h = w.hash().into();
            snap.get_withdrawal(&h)?
                .with_context(|| format!("block {} withdrawal {} not found", b, h.pack()))
        });
        extra_reqs.collect::<Result<Vec<_>>>()?
    };
    sync_server.publish_local_block(
        LocalBlock::new_builder()
            .block(block)
            .post_global_state(global_state)
            .deposit_info_vec(deposit_info_vec)
            .deposit_asset_scripts(ScriptVec::new_builder().set(deposit_asset_scripts).build())
            .withdrawals(withdrawals.pack())
            .build(),
    );
    Ok(())
}

fn publish_submitted(
    sync_server: &mut BlockSyncServerState,
    snap: &StoreSnapshot,
    b: u64,
) -> Result<()> {
    let block_hash = snap
        .get_block_hash_by_number(b)?
        .context("get block hash")?;
    let tx_hash = snap
        .get_block_submit_tx_hash(b)
        .context("get submit tx hash")?;
    sync_server.publish_submitted(
        Submitted::new_builder()
            .tx_hash(tx_hash.pack())
            .number_hash(
                NumberHash::new_builder()
                    .number(b.pack())
                    .block_hash(block_hash.pack())
                    .build(),
            )
            .build(),
    );
    Ok(())
}

fn publish_confirmed(
    sync_server: &mut BlockSyncServerState,
    snap: &StoreSnapshot,
    b: u64,
) -> Result<()> {
    let block_hash = snap
        .get_block_hash_by_number(b)?
        .context("get block hash")?;
    let tx_hash = snap
        .get_block_submit_tx_hash(b)
        .context("get submit tx hash")?;
    sync_server.publish_confirmed(
        Confirmed::new_builder()
            .tx_hash(tx_hash.pack())
            .number_hash(
                NumberHash::new_builder()
                    .number(b.pack())
                    .block_hash(block_hash.pack())
                    .build(),
            )
            .build(),
    );
    Ok(())
}
