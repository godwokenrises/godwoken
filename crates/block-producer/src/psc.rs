#![allow(clippy::mutable_key_type)]

use std::{collections::HashSet, fmt::Display, sync::Arc, time::Duration};

use anyhow::{bail, ensure, Context, Result};
use gw_chain::chain::Chain;
use gw_config::PscConfig;
use gw_mem_pool::{block_sync_server::BlockSyncServerState, pool::MemPool};
use gw_rpc_client::{
    error::{get_jsonrpc_error_code, CkbRpcError},
    rpc_client::RPCClient,
};
use gw_store::{snapshot::StoreSnapshot, traits::chain_store::ChainStore, Store};
use gw_telemetry::traits::{OpenTelemetrySpanExt, TraceContextExt};
use gw_types::{
    h256::*,
    offchain::{CellStatus, DepositInfo, TxStatus},
    packed::{
        self, Confirmed, GlobalState, LocalBlock, NumberHash, OutPoint, Revert, Script, ScriptVec,
        Submitted, Transaction, WithdrawalKey,
    },
    prelude::*,
};
use gw_utils::{
    abort_on_drop::spawn_abort_on_drop, liveness::Liveness, local_cells::LocalCellsManager,
    since::Since,
};
use tokio::{
    signal::unix::{signal, SignalKind},
    sync::Mutex,
    time::Instant,
};
use tracing::instrument;

use crate::{
    block_producer::{check_block_size, BlockProducer, ComposeSubmitTxArgs, TransactionSizeError},
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

impl ProduceSubmitConfirm {
    fn new(context: Arc<PSCContext>) -> Self {
        Self {
            context,
            local_count: 0,
            submitted_count: 0,
        }
    }

    fn set_local_count(&mut self, count: u64) {
        self.local_count = count;

        gw_metrics::block_producer().local_blocks.set(count);
        gw_metrics::custodian().finalized_custodians(&self.context.store);
    }

    fn set_submitted_count(&mut self, count: u64) {
        self.submitted_count = count;

        gw_metrics::block_producer().submitted_blocks.set(count);
    }
}

pub struct PSCContext {
    pub store: Store,
    pub rpc_client: RPCClient,
    pub chain: Arc<Mutex<Chain>>,
    pub mem_pool: Arc<Mutex<MemPool>>,
    pub block_producer: BlockProducer,
    pub local_cells_manager: Arc<Mutex<LocalCellsManager>>,
    pub chain_updater: ChainUpdater,
    pub rollup_type_script: Script,
    pub psc_config: PscConfig,
    pub block_sync_server_state: Option<Arc<std::sync::Mutex<BlockSyncServerState>>>,
    pub liveness: Arc<Liveness>,
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
    fn chain_updater(&self) -> &ChainUpdater {
        &self.chain_updater
    }
    fn rollup_type_script(&self) -> &Script {
        &self.rollup_type_script
    }
    fn liveness(&self) -> &Liveness {
        &self.liveness
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
        ensure!(last_submitted == last_confirmed);
        {
            let mut local_cells_manager = context.local_cells_manager.lock().await;
            for b in last_confirmed + 1..=last_valid {
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
        }

        log::info!(
            "last valid: {}, last_submitted: {}, last_confirmed: {}",
            last_valid,
            last_submitted,
            last_confirmed
        );

        let mut psc = Self::new(context);
        psc.set_local_count(last_valid - last_submitted);
        psc.set_submitted_count(last_submitted - last_confirmed);

        Ok(psc)
    }

    /// Run the producing, submitting and confirming loop.
    pub async fn run(mut self) -> Result<()> {
        loop {
            match run(&mut self).await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    log::warn!("{:#}", e);
                    if let Some(should_revert) = e.downcast_ref::<ShouldRevertError>() {
                        let revert_to = should_revert.0 - 1;
                        log::info!("revert to block {revert_to}");

                        let mut store_tx = self.context.store.begin_transaction();
                        revert(&*self.context, &mut store_tx, revert_to).await?;
                        store_tx.commit()?;
                    }
                    if e.is::<ShouldResyncError>() || e.is::<ShouldRevertError>() {
                        sync_l1(&*self.context).await?;

                        // Reset local_count, submitted_count and local_cells_manager.
                        let snap = self.context.store.get_snapshot();
                        let last_valid = snap.get_last_valid_tip_block()?.raw().number().unpack();
                        let last_submitted_nh = snap
                            .get_last_submitted_block_number_hash()
                            .expect("get last submitted");
                        let last_submitted = last_submitted_nh.number().unpack();
                        let last_confirmed = snap
                            .get_last_confirmed_block_number_hash()
                            .expect("get last confirmed")
                            .number()
                            .unpack();
                        ensure!(last_submitted == last_confirmed);
                        log::info!(
                            "last valid: {}, last_submitted: {}, last_confirmed: {}",
                            last_valid,
                            last_submitted,
                            last_confirmed
                        );
                        if let Some(ref sync_server) = self.context.block_sync_server_state {
                            let mut sync_server = sync_server.lock().unwrap();
                            sync_server.publish_revert(
                                Revert::new_builder().number_hash(last_submitted_nh).build(),
                            );
                            for b in last_confirmed + 1..=last_valid {
                                publish_local_block(&mut sync_server, &snap, b)?;
                            }
                        }

                        {
                            let mut local_cells_manager =
                                self.context.local_cells_manager.lock().await;
                            local_cells_manager.reset();
                            for b in last_confirmed + 1..=last_valid {
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
                        self.set_local_count(last_valid - last_submitted);
                        self.set_submitted_count(last_submitted - last_confirmed);
                    } else {
                        bail!(e);
                    }
                }
            }
        }
    }
}

async fn run(state: &mut ProduceSubmitConfirm) -> Result<()> {
    let mut submitting = false;
    let mut submit_handle = spawn_abort_on_drop(async { anyhow::Ok(NumberHash::default()) });
    let mut confirming = false;
    let mut confirm_handle = spawn_abort_on_drop(async { anyhow::Ok(NumberHash::default()) });
    let ctx = state.context.clone();
    let config = &ctx.psc_config;
    let mut interval = tokio::time::interval(Duration::from_secs(config.block_interval_secs));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let mut revert_local_signal = signal(SignalKind::user_defined1())?;
    let mut revert_submitted_signal = signal(SignalKind::user_defined2())?;

    loop {
        if !submitting && state.local_count > 0 && state.submitted_count < config.submitted_limit {
            submitting = true;
            let context = state.context.clone();
            submit_handle.replace_with(tokio::spawn(async move {
                loop {
                    match submit_next_block(&context).await {
                        Ok(nh) => return Ok(nh),
                        Err(err) => {
                            if err.is::<ShouldResyncError>() || err.is::<ShouldRevertError>() {
                                bail!(err);
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
                            if err.is::<ShouldResyncError>() || err.is::<ShouldRevertError>() {
                                bail!(err);
                            }
                            log::warn!("failed to confirm next block: {:#}", err);
                            // TOOO: backoff.
                            tokio::time::sleep(Duration::from_secs(3)).await;
                        }
                    }
                }
            }));
        }
        // One of the producing, submitting or confirming branch is always
        // enabled. Otherwise we'd be stuck waiting for one of the signals.
        assert!(state.local_count < config.local_limit || confirming || submitting);
        tokio::select! {
            biased;
            _ = revert_local_signal.recv() => {
                log::info!("revert not submitted blocks due to signal");
                let last_submitted = state
                    .context
                    .store
                    .get_last_submitted_block_number_hash()
                    .context("get last submitted")?
                    .number()
                    .unpack();
                bail!(ShouldRevertError(last_submitted));
            }
            _ = revert_submitted_signal.recv() => {
                log::info!("revert to last confirmed block due to signal");
                let last_confirmed = state
                    .context
                    .store
                    .get_last_confirmed_block_number_hash()
                    .context("get last submitted")?
                    .number()
                    .unpack();
                bail!(ShouldRevertError(last_confirmed));
            }
            // Block confirmed.
            result = &mut confirm_handle, if confirming => {
                confirming = false;
                match result {
                    Err(err) if err.is_panic() => bail!("sync task panic: {:?}", err.into_panic()),
                    Ok(nh) => {
                        let nh = nh?;
                        let mut store_tx = state.context.store.begin_transaction();
                        store_tx.set_last_confirmed_block_number_hash(&nh.as_reader())?;
                        store_tx.commit()?;
                        if let Some(ref sync_server) = state.context.block_sync_server_state {
                            let mut sync_server = sync_server.lock().unwrap();
                            publish_confirmed(&mut sync_server, &state.context.store.get_snapshot(), nh.number().unpack())?;
                        }
                        state.set_submitted_count(state.submitted_count - 1);
                    }
                    _ => {}
                }
            }
            // Block submitted.
            result = &mut submit_handle, if submitting => {
                submitting = false;
                match result {
                    Err(err) if err.is_panic() => bail!("submit task panic: {:?}", err.into_panic()),
                    Ok(nh) => {
                        let nh = nh?;
                        let mut store_tx = state.context.store.begin_transaction();
                        store_tx.set_last_submitted_block_number_hash(&nh.as_reader())?;
                        store_tx.commit()?;
                        if let Some(ref sync_server) = state.context.block_sync_server_state {
                            let mut sync_server = sync_server.lock().unwrap();
                            publish_submitted(&mut sync_server, &state.context.store.get_snapshot(), nh.number().unpack())?;
                        }
                        state.set_local_count(state.local_count - 1);
                        state.set_submitted_count(state.submitted_count + 1);
                    }
                    _ => {}
                }
            }
            // Produce a new local block if the produce timer has expired and
            // there are not too many local blocks.
            _ = interval.tick(), if state.local_count < config.local_limit => {
                log::info!("producing next block");
                if let Err(e) = produce_local_block(&state.context).await {
                    log::warn!("failed to produce local block: {:#}", e);
                } else {
                    state.set_local_count(state.local_count + 1);
                }
            }
        }
        // We have produced, submitted or confirmed a block. Update liveness tick.
        state.context.liveness.tick();
    }
}

/// Produce and save local block.
#[instrument(skip_all)]
async fn produce_local_block(ctx: &PSCContext) -> Result<()> {
    // TODO: check block and retry.

    // Lock mem pool the whole time we produce and update the next block. Don't
    // push transactions. Transactions pushed in this period of time will need
    // to be re-injected after the mem pool is reset anyway, and that creates a
    // quite some pressure on p2p syncing and read-only nodes.
    let mut pool = ctx.mem_pool.lock().await;

    let mut retry_count = 0;
    let ProduceBlockResult {
        block,
        global_state,
        withdrawal_extras,
        deposit_cells,
        remaining_capacity,
    } = loop {
        let result = ctx
            .block_producer
            .produce_next_block(&mut pool, retry_count)
            .await?;

        if check_block_size(result.block.as_slice().len()).is_ok() {
            break result;
        }
        retry_count += 1;
        log::warn!("block too large, retry {retry_count}");
    };

    let number: u64 = block.raw().number().unpack();
    let block_hash: H256 = block.hash();

    let block_txs = block.transactions().len();
    let block_withdrawals = block.withdrawals().len();

    // Now update db about the new local L2 block

    let deposit_info_vec = deposit_cells.pack();
    let deposit_asset_scripts: HashSet<Script> = deposit_cells
        .iter()
        .filter_map(|d| d.cell.output.type_().to_opt())
        .collect();

    // Publish block to read-only nodes as soon as possible, so that read-only nodes does not lag behind us.
    if let Some(ref sync_server_state) = ctx.block_sync_server_state {
        // Propagate tracing context.
        let cx = tracing::Span::current().context();
        let span_ref = cx.span();
        let span_context = span_ref.span_context();
        sync_server_state.lock().unwrap().publish_local_block(
            LocalBlock::new_builder()
                .trace_id(packed::Byte16::from_slice(&span_context.trace_id().to_bytes()).unwrap())
                .span_id(packed::Byte8::from_slice(&span_context.span_id().to_bytes()).unwrap())
                .block(block.clone())
                .post_global_state(global_state.clone())
                .deposit_info_vec(deposit_info_vec.clone())
                .deposit_asset_scripts(
                    ScriptVec::new_builder()
                        .extend(deposit_asset_scripts.iter().cloned())
                        .build(),
                )
                .withdrawals(withdrawal_extras.iter().cloned().pack())
                .build(),
        );
    }

    let mut chain = ctx.chain.lock().await;
    tokio::task::block_in_place(|| {
        let mut store_tx = ctx.store.begin_transaction();
        chain.update_local(
            &mut store_tx,
            block,
            deposit_info_vec,
            deposit_asset_scripts,
            withdrawal_extras,
            global_state,
        )?;
        log::info!(
            "produced new block #{} (txs: {}, deposits: {}, withdrawals: {}, capacity: {})",
            number,
            block_txs,
            deposit_cells.len(),
            block_withdrawals,
            remaining_capacity.capacity,
        );
        store_tx.set_block_post_finalized_custodian_capacity(
            number,
            &remaining_capacity.pack().as_reader(),
        )?;
        store_tx.commit()?;
        anyhow::Ok(())
    })?;
    drop(chain);

    // Lock collected deposits.
    let mut local_cells_manager = ctx.local_cells_manager.lock().await;
    for d in deposit_cells {
        local_cells_manager.lock_cell(d.cell.out_point);
    }

    pool.notify_new_tip(block_hash, &local_cells_manager)
        .await
        .expect("notify new tip");

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
    let last_confirmed = snap
        .get_last_confirmed_block_number_hash()
        .expect("get last confirmed block number and hash")
        .number()
        .unpack();
    // The first submission should not have any unknown cell error. If
    // it does, it means that previous block is probably not confirmed
    // anymore, and we should sync with L1 again.
    let is_first = block_number == last_confirmed + 1;
    submit_block(ctx, snap, is_first, block_number).await
}

#[instrument(skip(ctx, snap, is_first))]
async fn submit_block(
    ctx: &PSCContext,
    snap: StoreSnapshot,
    is_first: bool,
    block_number: u64,
) -> Result<NumberHash> {
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
        let tx = ctx
            .block_producer
            .compose_submit_tx(args)
            .await
            .map_err(|err| {
                if err.is::<TransactionSizeError>() {
                    err.context(ShouldRevertError(block_number))
                } else {
                    err
                }
            })?;

        let mut store_tx = ctx.store.begin_transaction();
        store_tx.set_block_submit_tx(block_number, &tx.as_reader())?;
        store_tx.commit()?;

        gw_metrics::block_producer()
            .tx_size
            .inc_by(tx.total_size() as u64);
        gw_metrics::block_producer()
            .witness_size
            .inc_by(tx.witnesses().total_size() as u64);

        log::info!("generated submission transaction");

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

    {
        // Deposits should be live.
        let deposits = ctx
            .store
            .get_block_deposit_info_vec(block_number)
            .context("get deposit info vec")?;
        for d in deposits {
            let out_point = d.cell().out_point();
            if !matches!(
                ctx.rpc_client
                    .get_cell(out_point.clone())
                    .await?
                    .map(|c| c.status),
                Some(CellStatus::Live)
            ) {
                bail!(anyhow::Error::new(ShouldRevertError(block_number))
                    .context(format!("deposit cell {} is no longer live", out_point)));
            }
        }
    }

    log::info!("sending transaction 0x{}", hex::encode(tx.hash()));
    if let Err(e) = send_transaction_or_check_inputs(&ctx.rpc_client, &tx).await {
        if e.is::<UnknownCellError>() {
            if is_first {
                bail!(e.context(ShouldResyncError));
            }
            bail!(e);
        } else if e.is::<DeadCellError>() {
            bail!(e.context(ShouldRevertError(block_number)));
        } else {
            bail!(e);
        }
    }
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
        let status = rpc_client.ckb.get_transaction_status(tx.hash()).await?;
        let should_resend = match status {
            Some(TxStatus::Committed) => break,
            Some(TxStatus::Rejected) => true,
            // Resend the transaction if it has been unknown, pending, or
            // proposed for some time. Or the transaction could be stuck in the
            // current state.
            //
            // This is also recommended by [CKB Transactions Management
            // Guideline](https://hackmd.io/@doitian/Sk8-gKX7D):
            //
            // > The generator must store the Pending transactions locally and
            // > send them to CKB nodes at regular intervals. It’s essential
            // > because CKB nodes may drop the transactions in their pools.
            _ => last_sent.elapsed() > Duration::from_secs(24),
        };
        if should_resend {
            log::info!("resend transaction 0x{}", hex::encode(tx.hash()));
            send_transaction_or_check_inputs(rpc_client, tx).await?;
            last_sent = Instant::now();
            gw_metrics::block_producer().resend.inc();
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    // Wait for indexer syncing the L1 block.
    let block_number = rpc_client
        .ckb
        .get_transaction_block_number(tx.hash())
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
    confirm_block(context, snap, block_number).await
}

#[instrument(skip(context, snap))]
async fn confirm_block(
    context: &PSCContext,
    snap: StoreSnapshot,
    block_number: u64,
) -> Result<NumberHash, anyhow::Error> {
    let block_hash = snap
        .get_block_hash_by_number(block_number)?
        .expect("block hash");
    let tx = snap
        .get_block_submit_tx(block_number)
        .expect("get submit tx");
    drop(snap);
    poll_tx_confirmed(&context.rpc_client, &tx)
        .await
        .map_err(|e| {
            if e.is::<UnknownCellError>() {
                e.context(ShouldResyncError)
            } else if e.is::<DeadCellError>() {
                e.context(ShouldRevertError(block_number))
            } else {
                e
            }
        })?;
    log::info!("block confirmed");
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
                    bail!(DeadCellError {
                        consumed_by_tx: Some(tx.hash()),
                    });
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
    bail!(DeadCellError {
        consumed_by_tx: None,
    });
}

/// Send transaction.
///
/// Will check input cells if sending fails with `TransactionFailedToResolve`.
/// If any input cell is dead, the error returned will be a `DeadCellError`.
async fn send_transaction_or_check_inputs(
    rpc_client: &RPCClient,
    tx: &Transaction,
) -> anyhow::Result<()> {
    if let Err(mut err) = rpc_client.send_transaction(tx).await {
        let code = get_jsonrpc_error_code(&err);
        if code == Some(CkbRpcError::TransactionFailedToResolve as i64) {
            if let Err(e) = check_tx_input(rpc_client, tx).await {
                // If the input is consumed by tx, tx is actually confirmed.
                // This can happen if the tx is confirmed right before it is
                // resent and its inputs is checked.
                if let Some(dead) = e.downcast_ref::<DeadCellError>() {
                    if dead.consumed_by_tx == Some(tx.hash()) {
                        return Ok(());
                    }
                }
                err = e.context(err);
                // Now, ∀T, e.is::<T>() -> err.is::<T>().
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

#[derive(Debug, thiserror::Error)]
#[error("should revert block {0}")]
struct ShouldRevertError(u64);

#[derive(Debug)]
struct ShouldResyncError;

impl Display for ShouldResyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "should resync")
    }
}

#[derive(thiserror::Error, Debug)]
struct DeadCellError {
    consumed_by_tx: Option<H256>,
}

impl Display for DeadCellError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "dead cell")?;
        if let Some(ref tx) = self.consumed_by_tx {
            write!(f, ": consumed by tx 0x{}", hex::encode(tx.as_slice()))?;
        }
        Ok(())
    }
}

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
        let asset_hashes: HashSet<H256> = reader
            .iter()
            .filter_map(|r| {
                let h: H256 = r.request().sudt_script_hash().unpack();
                if h.is_zero() {
                    None
                } else {
                    Some(h)
                }
            })
            .collect();
        let asset_scripts = asset_hashes.into_iter().map(|h| {
            snap.get_asset_script(&h)?
                .with_context(|| format!("block {} asset script {} not found", b, h.pack()))
        });
        asset_scripts.collect::<Result<Vec<_>>>()?
    };
    let withdrawals = {
        let reqs = block.as_reader().withdrawals();
        let extra_reqs = reqs.iter().map(|w| {
            let h = w.hash();
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
