use std::{sync::Arc, time::Duration};

use anyhow::{anyhow, bail, ensure, Result};
use gw_chain::chain::Chain;
use gw_common::H256;
use gw_mem_pool::pool::MemPool;
use gw_rpc_client::rpc_client::RPCClient;
use gw_store::{traits::chain_store::ChainStore, Store};
use gw_types::{
    offchain::{CellInfo, CellStatus, CollectedCustodianCells, DepositInfo, TxStatus},
    packed::{GlobalState, NumberHash, OutPoint, Transaction, WithdrawalKey},
    prelude::*,
};
use gw_utils::since::Since;
use tokio::{sync::Mutex, time::Instant};

use crate::{
    block_producer::{BlockProducer, ComposeSubmitTxArgs},
    local_cells::LocalCellsManager,
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
}

impl ProduceSubmitConfirm {
    pub async fn init(context: Arc<PSCContext>) -> Result<Self> {
        let store_tx = context.store.begin_transaction();
        let last_valid_block = store_tx.get_last_valid_tip_block()?;
        let last_valid_number_hash = NumberHash::new_builder()
            .number(last_valid_block.raw().number())
            .block_hash(last_valid_block.hash().pack())
            .build();
        let last_valid = last_valid_block.raw().number().unpack();
        // Set to last_valid because if it is None, it means we are
        // migrating from the version that does not decouple producing and
        // submitting or it's a new chain.
        let last_submitted = match store_tx.get_last_submitted_block_number_hash() {
            Some(b) => b.number().unpack(),
            None => {
                // Get rollup cell from indexer.
                let rollup_cell = context
                    .rpc_client
                    .query_rollup_cell()
                    .await?
                    .ok_or_else(|| anyhow!("failed to query rollup cell"))?;
                // TODO: make sure rollup cell data global state matches that of
                // last_valid. If it does not match, we need to sync with L1
                // state.
                store_tx.set_rollup_cell(last_valid, &rollup_cell.pack().as_reader())?;

                store_tx
                    .set_last_submitted_block_number_hash(&last_valid_number_hash.as_reader())?;
                last_valid
            }
        };
        let last_confirmed = match store_tx.get_last_confirmed_block_number_hash() {
            Some(b) => b.number().unpack(),
            None => {
                store_tx
                    .set_last_confirmed_block_number_hash(&last_valid_number_hash.as_reader())?;
                last_valid
            }
        };
        store_tx.commit()?;
        context.local_cells_manager.lock().await.refresh();
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
            // TODO: make this configurable.
            local_limit: 5,
            submitted_limit: 3,
        })
    }

    pub async fn run(self) -> Result<()> {
        run(self).await
    }
}

/// Run the producing, submitting and confirming loop.
async fn run(mut state: ProduceSubmitConfirm) -> Result<()> {
    let mut submitting = false;
    let mut submit_handle = tokio::spawn(async { NumberHash::default() });
    let mut syncing = false;
    let mut sync_handle = tokio::spawn(async { NumberHash::default() });
    let timer = tokio::time::sleep(Duration::from_secs(0));
    tokio::pin!(timer);
    loop {
        tokio::select! {
            // Produce a new local block if the produce timer has expired and
            // there are not too many local blocks.
            _ = &mut timer, if state.local_count < state.local_limit => {
                timer.as_mut().reset(Instant::now() + Duration::from_secs(3));
                log::info!("producing next block");
                if let Err(e) = produce_local_block(&state.context).await {
                    log::warn!("failed to produce local block: {:?}", e);
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
                        store_tx.set_last_submitted_block_number_hash(&nh.as_reader())?;
                        store_tx.commit()?;
                        state.context.local_cells_manager.lock().await.refresh();
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
                        store_tx.set_last_confirmed_block_number_hash(&nh.as_reader())?;
                        store_tx.commit()?;
                        state.context.local_cells_manager.lock().await.refresh();
                        // TODO: update L2 block committed info.
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
            submit_handle = tokio::spawn(async move {
                loop {
                    match submit_next_block(&context).await {
                        Ok(nh) => break nh,
                        Err(err) => {
                            log::warn!("failed to submit next block: {:?}", err);
                            // TOOO: backoff.
                            tokio::time::sleep(Duration::from_secs(20)).await;
                        }
                    }
                }
            });
        }
        if !syncing && state.submitted_count > 0 {
            syncing = true;
            let context = state.context.clone();
            sync_handle = tokio::spawn(async move {
                loop {
                    match sync_next_block(&context).await {
                        Ok(nh) => break nh,
                        Err(err) => {
                            log::warn!("failed to confirm next block: {:?}", err);
                            // TOOO: backoff.
                            tokio::time::sleep(Duration::from_secs(3)).await;
                        }
                    }
                }
            });
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
        finalized_custodians,
    } = ctx.block_producer.produce_next_block(0).await?;
    let number: u64 = block.raw().number().unpack();
    let block_hash: H256 = block.hash().into();

    let block_txs = block.transactions().len();
    let block_withdrawals = block.withdrawals().len();

    // Now update db about the new local L2 block

    let deposit_requests = deposit_cells.iter().map(|d| d.request.clone()).collect();
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
            deposit_requests,
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

    // Save additional information for composing the submit tx later.
    store_tx.set_block_deposit_info_vec(number, &deposit_cells.pack().as_reader())?;
    store_tx
        .set_block_collected_custodian_cells(number, &finalized_custodians.pack().as_reader())?;

    store_tx.commit()?;

    ctx.mem_pool.lock().await.notify_new_tip(block_hash).await?;

    // TODO??: update built-in web3_indexer

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
        .ok_or_else(|| anyhow!("failed to get next block hash"))?;
    let block = snap
        .get_block(&block_hash)?
        .ok_or_else(|| anyhow!("get_block"))?;
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
                .ok_or_else(|| anyhow!("get withdrawal"))?;
            anyhow::ensure!(extra.hash() == w.hash());
            withdrawal_extras.push(extra);
        }
        let deposit_cells: Vec<DepositInfo> = snap
            .get_block_deposit_info_vec(block_number)
            .ok_or_else(|| anyhow!("failed to get deposit info vec"))?
            .unpack();
        let global_state: GlobalState = snap
            .get_block_post_global_state(&block_hash)?
            .ok_or_else(|| anyhow!("failed to get block global_state"))?;
        let finalized_custodians: CollectedCustodianCells = snap
            .get_block_collected_custodian_cells(block_number)
            .ok_or_else(|| anyhow!("failed to get block collected custodian cells"))?
            .as_reader()
            .unpack();
        let rollup_cell: CellInfo = snap
            .get_rollup_cell_info(block_number - 1)
            .ok_or_else(|| anyhow!("get rollup cell"))?
            .as_reader()
            .unpack();
        drop(snap);

        let payment_cells_manager = ctx.local_cells_manager.lock().await;

        let args = ComposeSubmitTxArgs {
            deposit_cells,
            finalized_custodians,
            block,
            global_state,
            since,
            rollup_cell,
            withdrawal_extras,
            local_consumed_cells: payment_cells_manager.local_consumed(),
            local_live_payment_cells: payment_cells_manager.local_live_payment(),
            local_live_stake_cells: payment_cells_manager.local_live_stake(),
        };
        let (tx, next_rollup_cell) = ctx.block_producer.compose_submit_tx(args).await?;

        let store_tx = ctx.store.begin_transaction();
        store_tx.set_submit_tx(block_number, &tx.as_reader())?;
        store_tx.set_rollup_cell(block_number, &next_rollup_cell.pack().as_reader())?;
        store_tx.commit()?;

        log::info!(
            "generated submission transaction for block {}",
            block_number
        );

        tx
    };

    // Wait until median >= since, or CKB will reject the transaction.
    loop {
        match median_gte(&ctx.rpc_client, since_millis).await {
            Ok(_) => break,
            Err(err) => {
                log::info!("wait for median >= {}: {:?}", since_millis, err);
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }
    }

    // TODO: Some error can be ignored. Some error we cannot recover from, e.g.
    // a deposit cell is dead after an L1 reorg. We may need to re-generate the
    // block in such cases.
    log::info!(
        "sending transaction 0x{} to submit block {}",
        hex::encode(tx.hash()),
        block_number
    );
    if let Err(e) = ctx.rpc_client.send_transaction(&tx).await {
        if e.to_string().contains("TransactionFailedToResolve") {
            if let Err(e) = check_tx_input(&ctx.rpc_client, &tx).await {
                log::error!("tx input error: {:?}", e);
            } else {
                log::error!("but tx input is all live");
            }
        } else {
            log::error!("send tx {:?}", e);
        }
        return Err(e);
    }
    log::info!("transaction sent");
    Ok(NumberHash::new_builder()
        .block_hash(block_hash.pack())
        .number(block_number.pack())
        .build())
}

async fn poll_tx_confirmed(rpc_client: &RPCClient, tx: &Transaction) -> Result<()> {
    log::info!("waiting for tx 0x{}", hex::encode(tx.hash()));
    let mut last_sent = Instant::now();
    loop {
        tokio::time::sleep(Duration::from_secs(3)).await;
        let status = rpc_client.get_transaction_status(tx.hash().into()).await?;
        match status {
            Some(TxStatus::Pending) => {}
            Some(TxStatus::Proposed) => {}
            Some(TxStatus::Committed) => break Ok(()),
            None => {
                // Resend the transaction if get_transaction returns null after 20 seconds.
                if last_sent.elapsed() > Duration::from_secs(20) {
                    log::info!("resend transaction 0x{}", hex::encode(tx.hash()));
                    if let Err(e) = rpc_client.send_transaction(tx).await {
                        if e.to_string().contains("TransactionFailedToResolve") {
                            if let Err(e) = check_tx_input(rpc_client, tx).await {
                                log::error!("tx input error: {:?}", e);
                            } else {
                                log::error!("but tx input is all live");
                            }
                        } else {
                            log::error!("send tx {:?}", e);
                        }
                    }
                    last_sent = Instant::now();
                }
            }
        }
    }
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
    poll_tx_confirmed(&context.rpc_client, &tx).await?;
    log::info!("block {} confirmed", block_number);
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

async fn check_cell(rpc_client: &RPCClient, out_point: OutPoint) -> Result<()> {
    let block_number = rpc_client
        .get_transaction_block_number(out_point.tx_hash().unpack())
        .await?
        .ok_or_else(|| anyhow!("transaction not committed"))?;
    let mut opt_block = rpc_client.get_block_by_number(block_number).await?;
    // Search later blocks to see who consumed this cell.
    loop {
        if let Some(block) = opt_block {
            for tx in block.transactions() {
                if tx
                    .raw()
                    .inputs()
                    .into_iter()
                    .any(|i| i.previous_output() == out_point)
                {
                    bail!(
                        "OutPoint {:?} is consumed by tx 0x{}",
                        out_point,
                        hex::encode(tx.hash())
                    );
                }
            }
            opt_block = rpc_client
                .get_block_by_number(block.header().raw().number().unpack() + 1)
                .await?;
        } else {
            return Ok(());
        }
    }
}

async fn check_tx_input(rpc_client: &RPCClient, tx: &Transaction) -> Result<()> {
    // Check inputs.
    for input in tx.raw().inputs() {
        let out_point = input.previous_output();
        let status = rpc_client
            .get_cell(out_point.clone())
            .await?
            .map(|c| c.status);
        if status != Some(CellStatus::Live) {
            check_cell(rpc_client, out_point).await?;
        }
    }
    Ok(())
}
