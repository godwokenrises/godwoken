//! L1 and P2P block sync.

use std::{sync::Arc, time::Duration};

use anyhow::{ensure, Context, Result};
use bytes::Bytes;
use ckb_types::prelude::{Builder, Entity, Reader};
use futures::TryStreamExt;
use gw_chain::chain::Chain;
use gw_generator::generator::CyclesPool;
use gw_mem_pool::pool::MemPool;
use gw_p2p_network::{FnSpawn, P2P_SYNC_PROTOCOL, P2P_SYNC_PROTOCOL_NAME};
use gw_rpc_client::rpc_client::RPCClient;
use gw_store::{traits::chain_store::ChainStore, Store};
use gw_types::{
    packed::{
        BlockSync, BlockSyncReader, BlockSyncUnion, NumberHash, P2PSyncRequest,
        P2PSyncResponseReader, P2PSyncResponseUnionReader, Script,
    },
    prelude::Unpack,
};
use gw_utils::compression::StreamDecoder;
use tentacle::{
    builder::MetaBuilder,
    service::{ProtocolMeta, ServiceAsyncControl},
    SessionId, SubstreamReadPart,
};
use tokio::{
    sync::{
        mpsc::{UnboundedReceiver, UnboundedSender},
        Mutex,
    },
    task::block_in_place,
};

use crate::{
    chain_updater::ChainUpdater,
    sync_l1::{revert, sync_l1, SyncL1Context},
};

pub struct BlockSyncClient {
    pub store: Store,
    pub rpc_client: RPCClient,
    pub chain: Arc<Mutex<Chain>>,
    pub mem_pool: Option<Arc<Mutex<MemPool>>>,
    pub chain_updater: ChainUpdater,
    pub rollup_type_script: Script,
    pub p2p_stream_receiver: Option<UnboundedReceiver<P2PStream>>,
    pub completed_initial_syncing: bool,
}

impl SyncL1Context for BlockSyncClient {
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
}

impl BlockSyncClient {
    pub async fn run(mut self) {
        let mut p2p_stream = None;
        loop {
            if let Some(ref mut s) = p2p_stream {
                if let Err(err) = run_with_p2p_stream(&mut self, s).await {
                    if !err.is::<RecoverableCtx>() {
                        let _ = s.disconnect().await;
                        p2p_stream = None;
                    }
                    log::warn!("{:#}", err);
                }
                // TODO: backoff.
                tokio::time::sleep(Duration::from_secs(3)).await;
            } else {
                if let Some(ref mut receiver) = self.p2p_stream_receiver {
                    if let Ok(stream) = receiver.try_recv() {
                        p2p_stream = Some(stream);
                        continue;
                    }
                }
                if let Err(err) = run_once_without_p2p_stream(&mut self).await {
                    log::warn!("{:#}", err);
                }

                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }
    }
}

#[derive(Debug)]
struct RecoverableCtx;

impl std::fmt::Display for RecoverableCtx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "stream error")
    }
}

async fn run_once_without_p2p_stream(client: &mut BlockSyncClient) -> Result<()> {
    sync_l1(client).await?;
    notify_new_tip(client, true).await?;
    Ok(())
}

async fn run_with_p2p_stream(client: &mut BlockSyncClient, stream: &mut P2PStream) -> Result<()> {
    loop {
        sync_l1(client).await.context(RecoverableCtx)?;
        notify_new_tip(client, false)
            .await
            .context(RecoverableCtx)?;
        let last_confirmed = client
            .store
            .get_last_confirmed_block_number_hash()
            .context("last confirmed")?;
        log::info!("request syncing from {}", last_confirmed.number().unpack());
        let request = P2PSyncRequest::new_builder()
            .block_hash(last_confirmed.block_hash())
            .block_number(last_confirmed.number())
            .build();
        stream.send(request.as_bytes()).await?;
        let response = stream.recv().await?.context("unexpected end of stream")?;
        let response = P2PSyncResponseReader::from_slice(&response)?;
        match response.to_enum() {
            P2PSyncResponseUnionReader::Found(_) => break,
            P2PSyncResponseUnionReader::TryAgain(_) => {}
        }
        log::info!("will try again");
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    log::info!("receiving block sync messages from peer");
    while let Some(msg) = stream.recv().await? {
        BlockSyncReader::from_slice(msg.as_ref())?;
        let msg = BlockSync::new_unchecked(msg);
        apply_msg(client, msg).await?;
    }
    log::info!("end receiving block sync messages from peer");

    Ok(())
}

async fn apply_msg(client: &mut BlockSyncClient, msg: BlockSync) -> Result<()> {
    match msg.to_enum() {
        BlockSyncUnion::Revert(r) => {
            log::info!(
                "received revert block {}",
                r.number_hash().number().unpack()
            );

            check_number_hash(client, &r.number_hash())?;

            let store_tx = client.store.begin_transaction();
            let nh = r.number_hash();
            let nh = &nh.as_reader();
            store_tx.set_last_confirmed_block_number_hash(nh)?;
            store_tx.set_last_submitted_block_number_hash(nh)?;
            store_tx.commit()?;
        }
        BlockSyncUnion::LocalBlock(l) => {
            let block_hash = l.block().hash();
            let block_number = l.block().raw().number().unpack();
            log::info!(
                "received block {block_number} {}",
                ckb_types::H256::from(block_hash),
            );
            let store_tx = client.store.begin_transaction();
            let store_block_hash = store_tx.get_block_hash_by_number(block_number)?;
            if let Some(store_block_hash) = store_block_hash {
                if store_block_hash != block_hash.into() {
                    log::info!("revert to {}", block_number - 1);
                    revert(client, &store_tx, block_number - 1).await?;
                    store_tx.commit()?;
                } else {
                    log::info!("block already known");
                    return Ok(());
                }
            }
            {
                log::info!("update local block");
                let mut chain = client.chain.lock().await;
                block_in_place(|| {
                    chain.update_local(
                        &store_tx,
                        l.block(),
                        l.deposit_info_vec(),
                        l.deposit_asset_scripts().into_iter().collect(),
                        l.withdrawals().into_iter().collect(),
                        l.post_global_state(),
                    )?;
                    chain.calculate_and_store_finalized_custodians(&store_tx, block_number)?;
                    store_tx.commit()?;
                    anyhow::Ok(())
                })?;
            }
            notify_new_tip(client, false).await?;
        }
        BlockSyncUnion::Submitted(s) => {
            log::info!(
                "received submitted block {}",
                s.number_hash().number().unpack()
            );

            check_number_hash(client, &s.number_hash())?;

            let store_tx = client.store.begin_transaction();
            store_tx.set_block_submit_tx_hash(
                s.number_hash().number().unpack(),
                &s.tx_hash().unpack(),
            )?;
            store_tx.set_last_submitted_block_number_hash(&s.number_hash().as_reader())?;
            store_tx.commit()?;
        }
        BlockSyncUnion::Confirmed(c) => {
            log::info!(
                "received confirmed block {}",
                c.number_hash().number().unpack()
            );

            check_number_hash(client, &c.number_hash())?;

            let store_tx = client.store.begin_transaction();
            store_tx.set_last_confirmed_block_number_hash(&c.number_hash().as_reader())?;
            store_tx.commit()?;
        }
        BlockSyncUnion::NextMemBlock(m) => {
            log::info!("received mem block {}", m.block_info().number().unpack());
            if let Some(ref mem_pool) = client.mem_pool {
                let mut mem_pool = mem_pool.lock().await;
                let result = mem_pool
                    .refresh_mem_block(
                        m.block_info(),
                        m.withdrawals().into_iter().collect(),
                        m.deposits().unpack(),
                    )
                    .await;
                if let Err(err) = result {
                    log::warn!("{:#}", err);
                }
            }
        }
        BlockSyncUnion::L2Transaction(tx) => {
            log::info!("received L2Transaction 0x{}", hex::encode(tx.hash()));
            if let Some(ref mem_pool) = client.mem_pool {
                let mut mem_pool = mem_pool.lock().await;
                let mem_block_config = mem_pool.config();
                *mem_pool.cycles_pool_mut() = CyclesPool::new(
                    mem_block_config.max_cycles_limit,
                    mem_block_config.syscall_cycles.clone(),
                );

                let result = mem_pool.push_transaction(tx);
                if let Err(err) = result {
                    log::warn!("{:#}", err);
                }
            }
        }
    }
    Ok(())
}

pub struct P2PStream {
    id: SessionId,
    control: ServiceAsyncControl,
    read_part: SubstreamReadPart,
    decoder: StreamDecoder,
}

impl P2PStream {
    async fn recv(&mut self) -> Result<Option<Bytes>> {
        Ok(if let Some(msg) = self.read_part.try_next().await? {
            // Decompress message.
            Some(self.decoder.decode(&msg)?.into())
        } else {
            None
        })
    }

    async fn send(&mut self, msg: Bytes) -> Result<()> {
        self.control
            .send_message_to(self.id, P2P_SYNC_PROTOCOL, msg)
            .await?;
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        self.control.disconnect(self.id).await?;
        Ok(())
    }
}

// XXX: would unbounded channel leak memory?
/// The p2p protocol just sends the p2p stream to the client.
pub fn block_sync_client_protocol(stream_tx: UnboundedSender<P2PStream>) -> ProtocolMeta {
    let spawn = FnSpawn(move |context, control, read_part| {
        let control = control.clone();
        let id = context.id;
        let stream = P2PStream {
            id,
            control,
            read_part,
            decoder: StreamDecoder::new(),
        };
        let _ = stream_tx.send(stream);
    });
    MetaBuilder::new()
        .name(|_| P2P_SYNC_PROTOCOL_NAME.into())
        .id(P2P_SYNC_PROTOCOL)
        .protocol_spawn(spawn)
        .build()
}

async fn notify_new_tip(client: &mut BlockSyncClient, update_state: bool) -> Result<()> {
    if !client.completed_initial_syncing {
        if let Some(ref mem_pool) = client.mem_pool {
            let mut mem_pool = mem_pool.lock().await;
            let new_tip = client.store.get_last_valid_tip_block_hash()?;
            mem_pool.reset_read_only(Some(new_tip), update_state)?;
            mem_pool.mem_pool_state().set_completed_initial_syncing();
        }
        client.completed_initial_syncing = true;
    } else if let Some(ref mem_pool) = client.mem_pool {
        let mut mem_pool = mem_pool.lock().await;
        let new_tip = client.store.get_last_valid_tip_block_hash()?;
        mem_pool.reset_read_only(Some(new_tip), update_state)?;
    }
    Ok(())
}

fn check_number_hash(client: &BlockSyncClient, number_hash: &NumberHash) -> Result<()> {
    // Check block hash.
    let number = number_hash.number().unpack();
    let store_block_hash = client
        .store
        .get_block_hash_by_number(number)?
        .context("get block hash")?;
    ensure!(store_block_hash.as_slice() == number_hash.block_hash().as_slice());
    Ok(())
}
