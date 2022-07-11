#![allow(clippy::mutable_key_type)]

use std::{fmt::Display, sync::Arc, time::Duration};

use anyhow::{bail, ensure, Context, Result};
use gw_chain::chain::{Chain, RevertedL1Action};
use gw_common::H256;
use gw_jsonrpc_types::ckb_jsonrpc_types::BlockNumber;
use gw_mem_pool::pool::MemPool;
use gw_rpc_client::{
    error::{get_jsonrpc_error_code, CkbRpcError},
    indexer_types::{Order, SearchKey, SearchKeyFilter},
    rpc_client::RPCClient,
};
use gw_store::{traits::chain_store::ChainStore, transaction::StoreTransaction, Store};
use gw_types::{
    offchain::{CellStatus, DepositInfo, TxStatus},
    packed::{GlobalState, NumberHash, OutPoint, Script, Transaction, WithdrawalKey},
    prelude::*,
};
use gw_utils::{abort_on_drop::AbortOnDropHandle, local_cells::LocalCellsManager, since::Since};
use tokio::{sync::Mutex, time::Instant};

use crate::{
    block_producer::{BlockProducer, ComposeSubmitTxArgs},
    chain_updater::ChainUpdater,
    produce_block::ProduceBlockResult,
};

/// Block producing, submitting and confirming state machine.
pub struct ProduceSubmitConfirm {
    context: Arc<PSCContext>,
    local_count: u64,
    local_limit: u64,
    submitted_count: u64,
    submitted_limit: u64,
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
}

impl ProduceSubmitConfirm {
    pub async fn init(context: Arc<PSCContext>) -> Result<Self> {
        sync_l1(&context).await?;
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
                let tx = snap.get_submit_tx(b).expect("submit tx");
                local_cells_manager.apply_tx(&tx.as_reader());
            }
            for b in last_submitted + 1..=last_valid {
                let deposits = snap.get_block_deposit_info_vec(b).expect("deposit info");
                let deposits = deposits.into_iter().map(|d| d.cell());
                for c in deposits {
                    local_cells_manager.lock_cell(c.out_point());
                }
            }
            context
                .mem_pool
                .lock()
                .await
                .notify_new_tip(snap.get_last_valid_tip_block_hash()?, &local_cells_manager)
                .await?;
        }
        log::info!(
            "last valid: {}, last_submitted: {}, last_confirmed: {}",
            last_valid,
            last_submitted,
            last_confirmed
        );
        context
            .chain
            .lock()
            .await
            .complete_initial_syncing()
            .await?;
        let local_count = last_valid - last_submitted;
        let submitted_count = last_submitted - last_confirmed;
        Ok(Self {
            context,
            local_count,
            submitted_count,
            // TODO: make this configurable.
            local_limit: 3,
            submitted_limit: 5,
        })
    }

    /// Run the producing, submitting and confirming loop.
    pub async fn run(mut self) -> Result<()> {
        loop {
            match run(&mut self).await {
                Ok(()) => return Ok(()),
                Err(e) if e.is::<DeadCellError>() => {
                    log::warn!("Error: {:#}", e);

                    let error_block = e.downcast::<BlockContext>()?.0;
                    {
                        let store_tx = self.context.store.begin_transaction();
                        log::info!("revert to block {}", error_block - 1);
                        revert(&self.context, &store_tx, error_block - 1).await?;
                        store_tx.commit()?;
                    }

                    sync_l1(&self.context).await?;

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
                            let tx = snap.get_submit_tx(b).expect("submit tx");
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
    let submit_handle = tokio::spawn(async { anyhow::Ok(NumberHash::default()) });
    let mut submit_handle = AbortOnDropHandle::from(submit_handle);
    let mut syncing = false;
    let sync_handle = tokio::spawn(async { anyhow::Ok(NumberHash::default()) });
    let mut sync_handle = AbortOnDropHandle::from(sync_handle);
    let mut interval = tokio::time::interval(Duration::from_secs(3));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            // Produce a new local block if the produce timer has expired and
            // there are not too many local blocks.
            _ = interval.tick(), if state.local_count < state.local_limit => {
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
                        let store_tx = state.context.store.begin_transaction();
                        store_tx.set_last_submitted_block_number_hash(&nh?.as_reader())?;
                        store_tx.commit()?;
                        state.submitted_count += 1;
                        state.local_count -= 1;
                    }
                    _ => {}
                }
            }
            // Block confirmed.
            result = &mut sync_handle, if syncing => {
                syncing = false;
                match result {
                    Err(err) if err.is_panic() => bail!("sync task panic: {:?}", err.into_panic()),
                    Ok(nh) => {
                        let store_tx = state.context.store.begin_transaction();
                        store_tx.set_last_confirmed_block_number_hash(&nh?.as_reader())?;
                        store_tx.commit()?;
                        state.submitted_count -= 1;
                    }
                    _ => {}
                }
            }
            else => {}
        }
        if !submitting && state.local_count > 0 && state.submitted_count < state.submitted_limit {
            submitting = true;
            let context = state.context.clone();
            submit_handle.replace_with(tokio::spawn(async move {
                loop {
                    match submit_next_block(&context).await {
                        Ok(nh) => return Ok(nh),
                        Err(err) => {
                            if err.is::<DeadCellError>() {
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
        if !syncing && state.submitted_count > 0 {
            syncing = true;
            let context = state.context.clone();
            sync_handle.replace_with(tokio::spawn(async move {
                loop {
                    match sync_next_block(&context).await {
                        Ok(nh) => break Ok(nh),
                        Err(err) => {
                            if err.is::<DeadCellError>() {
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
    let tx = if let Some(tx) = snap.get_submit_tx(block_number) {
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
        store_tx.set_submit_tx(block_number, &tx.as_reader())?;
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
    send_transaction_or_check_inputs(&ctx.rpc_client, &tx)
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
            send_transaction_or_check_inputs(rpc_client, tx).await?;
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

async fn sync_next_block(context: &PSCContext) -> Result<NumberHash> {
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
    let tx = snap.get_submit_tx(block_number).expect("get submit tx");
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
        .context("transaction not committed")?;
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
async fn send_transaction_or_check_inputs(
    rpc_client: &RPCClient,
    tx: &Transaction,
) -> anyhow::Result<()> {
    if let Err(mut err) = rpc_client.send_transaction(tx).await {
        let code = get_jsonrpc_error_code(&err);
        if code == Some(CkbRpcError::TransactionFailedToResolve as i64) {
            if let Err(e) = check_tx_input(rpc_client, tx).await {
                err = e.context(err);
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

#[derive(thiserror::Error, Debug)]
#[error("dead cell")]
struct DeadCellError;

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

/// Sync with L1.
///
/// Will reset last confirmed, last submitted and last valid blocks.
async fn sync_l1(psc: &PSCContext) -> Result<()> {
    let store_tx = psc.store.begin_transaction();
    let last_confirmed_local = store_tx
        .get_last_confirmed_block_number_hash()
        .context("get last confirmed")?;
    let mut last_confirmed_l1 = last_confirmed_local.number().unpack();
    // Find last known block on L1.
    loop {
        log::info!("checking L2 block {last_confirmed_l1} on L1");
        let tx = psc
            .store
            .get_submit_tx(last_confirmed_l1)
            .context("get submit tx")?;
        if let Some(TxStatus::Committed) = psc
            .rpc_client
            .ckb
            .get_transaction_status(tx.hash().into())
            .await?
        {
            log::info!("L2 block {last_confirmed_l1} is on L1");
            break;
        }
        last_confirmed_l1 -= 1;
        if last_confirmed_l1 == 0 {
            break;
        }
    }

    sync_l1_unknown(psc, store_tx, last_confirmed_l1).await?;

    Ok(())
}

// Sync unknown blocks from L1.
//
// Although a L2 fork is highly unlikely, it is not impossible, due to e.g.
// accidentally running two godwoken full nodes.
async fn sync_l1_unknown(
    psc: &PSCContext,
    store_tx: StoreTransaction,
    last_confirmed: u64,
) -> Result<()> {
    log::info!("syncing unknown L2 blocks from L1");

    // Get submission transactions, if there are unknown transactions, revert, update.
    let tx = store_tx
        .get_submit_tx(last_confirmed)
        .context("get submit tx")?;
    let start_l1_block = psc
        .rpc_client
        .ckb
        .get_transaction_block_number(tx.hash().into())
        .await?
        .context("get transaction block number")?;
    let search_key =
        SearchKey::with_type(psc.rollup_type_script.clone()).with_filter(Some(SearchKeyFilter {
            block_range: Some([
                // Start from the same block containing the last confirmed tx,
                // because there may be other transactions in the same block.
                BlockNumber::from(start_l1_block),
                BlockNumber::from(u64::max_value()),
            ]),
            ..Default::default()
        }));
    let mut last_cursor = None;
    let last_confirmed_tx_hash = tx.hash().into();
    let mut seen_last_confirmed = false;
    let mut reverted = false;
    loop {
        let mut txs = psc
            .rpc_client
            .indexer
            .get_transactions(&search_key, &Order::Asc, None, &last_cursor)
            .await?;
        txs.objects.dedup_by_key(|obj| obj.tx_hash.clone());
        if txs.objects.is_empty() {
            break;
        }
        last_cursor = Some(txs.last_cursor);

        for tx in txs.objects {
            if !seen_last_confirmed {
                log::info!("skipping transaction {}", tx.tx_hash);
                if tx.tx_hash == last_confirmed_tx_hash {
                    seen_last_confirmed = true;
                }
                continue;
            }

            log::info!("syncing L1 transaction {}", tx.tx_hash);
            // TODO: we may get transactions that are submitted but not yet
            // confirmed from last run. In this case, we should simply mark the
            // corresponding block as confirmed instead of reverting.
            if !reverted {
                log::info!("L2 fork detected, reverting to L2 block {last_confirmed}");
                revert(psc, &store_tx, last_confirmed).await?;
                // Commit transaction because chain_updater.update_single will open and commit new transactions.
                store_tx.commit()?;
                reverted = true;
            }
            psc.chain_updater.update_single(&tx.tx_hash).await?;
        }
    }
    if !reverted {
        // Reset last confirmed.
        let block_hash = store_tx
            .get_block_hash_by_number(last_confirmed)?
            .context("get block hash")?;
        let nh = NumberHash::new_builder()
            .number(last_confirmed.pack())
            .block_hash(block_hash.pack())
            .build();
        store_tx.set_last_confirmed_block_number_hash(&nh.as_reader())?;
        store_tx.commit()?;
    }

    Ok(())
}

/// Revert L2 blocks.
async fn revert(
    psc: &PSCContext,
    store_tx: &StoreTransaction,
    revert_to_last_valid: u64,
) -> Result<()> {
    let mut chain = psc.chain.lock().await;
    loop {
        let block = store_tx.get_last_valid_tip_block()?;
        let block_number = block.raw().number().unpack();
        if block_number <= revert_to_last_valid {
            break;
        }
        let prev_global_state = store_tx
            .get_block_post_global_state(&block.raw().parent_block_hash().unpack())?
            .context("get parent global state")?;
        let action = RevertedL1Action {
            prev_global_state,
            context: gw_chain::chain::RevertL1ActionContext::SubmitValidBlock { l2block: block },
        };
        log::info!("reverting L2 block {}", block_number);
        chain.revert_l1action(store_tx, action)?;
    }

    Ok(())
}
