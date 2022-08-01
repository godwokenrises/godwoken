use anyhow::{Context, Result};
use gw_chain::chain::{Chain, RevertedL1Action};
use gw_jsonrpc_types::ckb_jsonrpc_types::BlockNumber;
use gw_rpc_client::{
    indexer_types::{Order, SearchKey, SearchKeyFilter},
    rpc_client::RPCClient,
};
use gw_store::{traits::chain_store::ChainStore, transaction::StoreTransaction, Store};
use gw_types::{
    offchain::TxStatus,
    packed::{NumberHash, Script},
    prelude::*,
};
use tokio::sync::Mutex;

use crate::chain_updater::ChainUpdater;

pub trait SyncL1Context {
    fn store(&self) -> &Store;
    fn rpc_client(&self) -> &RPCClient;
    fn chain(&self) -> &Mutex<Chain>;
    fn chain_updater(&self) -> &ChainUpdater;
    fn rollup_type_script(&self) -> &Script;
}

/// Sync with L1.
///
/// Will reset last confirmed, last submitted and last valid blocks.
pub async fn sync_l1(ctx: &(dyn SyncL1Context + Sync + Send)) -> Result<()> {
    log::info!("syncing with L1");
    let store_tx = ctx.store().begin_transaction();
    let last_confirmed_local = store_tx
        .get_last_confirmed_block_number_hash()
        .context("get last confirmed")?;
    let mut last_confirmed_l1 = last_confirmed_local.number().unpack();
    // Find last known block on L1.
    loop {
        log::info!("checking L2 block {last_confirmed_l1} on L1");
        let tx_hash = ctx
            .store()
            .get_block_submit_tx_hash(last_confirmed_l1)
            .context("get submit tx")?;
        if let Some(TxStatus::Committed) =
            ctx.rpc_client().ckb.get_transaction_status(tx_hash).await?
        {
            log::info!("L2 block {last_confirmed_l1} is on L1");
            break;
        }
        last_confirmed_l1 -= 1;
        if last_confirmed_l1 == 0 {
            break;
        }
    }

    // Try to confirm more blocks. Blocks submitted earlier (before restarting
    // this godwoken node) may be confirmed now.
    confirm_blocks(ctx, &store_tx, &mut last_confirmed_l1).await?;

    sync_l1_unknown(ctx, store_tx, last_confirmed_l1).await?;

    Ok(())
}

async fn confirm_blocks(
    ctx: &(dyn SyncL1Context + Send + Sync),
    store_tx: &StoreTransaction,
    last_confirmed: &mut u64,
) -> Result<()> {
    log::info!("confirm blocks");
    loop {
        let next = *last_confirmed + 1;
        if let Some(tx_hash) = store_tx.get_block_submit_tx_hash(next) {
            log::info!("try to confirme block {next}");
            match ctx.rpc_client().ckb.get_transaction_status(tx_hash).await? {
                Some(TxStatus::Committed) => {
                    log::info!("block {next} confirmed");
                }
                _ => break,
            }
        } else {
            break;
        }
        *last_confirmed = next;
    }
    Ok(())
}

// Sync unknown blocks from L1.
//
// Although a L2 fork is highly unlikely, it is not impossible, due to e.g.
// accidentally running two godwoken full nodes.
async fn sync_l1_unknown(
    ctx: &(dyn SyncL1Context + Send + Sync),
    store_tx: StoreTransaction,
    last_confirmed: u64,
) -> Result<()> {
    log::info!("syncing unknown L2 blocks from L1");

    // Get submission transactions, if there are unknown transactions, revert, update.
    let tx_hash = store_tx
        .get_block_submit_tx_hash(last_confirmed)
        .context("get submit tx")?;
    let start_l1_block = ctx
        .rpc_client()
        .ckb
        .get_transaction_block_number(tx_hash)
        .await?
        .context("get transaction block number")?;
    let search_key =
        SearchKey::with_type(ctx.rollup_type_script().clone()).with_filter(Some(SearchKeyFilter {
            block_range: Some([
                // Start from the same block containing the last confirmed tx,
                // because there may be other transactions in the same block.
                BlockNumber::from(start_l1_block),
                BlockNumber::from(u64::max_value()),
            ]),
            ..Default::default()
        }));
    let mut last_cursor = None;
    let tx_hash: [u8; 32] = tx_hash.into();
    let last_confirmed_tx_hash = tx_hash.into();
    let mut seen_last_confirmed = false;
    let mut reverted = false;
    loop {
        let mut txs = ctx
            .rpc_client()
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
            if !reverted {
                log::info!("L2 fork detected, reverting to L2 block {last_confirmed}");
                revert(ctx, &store_tx, last_confirmed).await?;
                // Commit transaction because chain_updater.update_single will open and commit new transactions.
                store_tx.commit()?;
                reverted = true;
            }
            ctx.chain_updater().update_single(&tx.tx_hash).await?;
        }
    }
    if !reverted {
        // Reset last confirmed and last_submitted.
        let block_hash = store_tx
            .get_block_hash_by_number(last_confirmed)?
            .context("get block hash")?;
        let nh = NumberHash::new_builder()
            .number(last_confirmed.pack())
            .block_hash(block_hash.pack())
            .build();
        store_tx.set_last_confirmed_block_number_hash(&nh.as_reader())?;
        store_tx.set_last_submitted_block_number_hash(&nh.as_reader())?;
        store_tx.commit()?;
    }

    Ok(())
}

/// Revert L2 blocks.
pub async fn revert(
    ctx: &(dyn SyncL1Context + Send + Sync),
    store_tx: &StoreTransaction,
    revert_to_last_valid: u64,
) -> Result<()> {
    let mut chain = ctx.chain().lock().await;
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
