//! L1 and P2P block sync.

use std::{collections::VecDeque, sync::Arc, time::Duration};

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
use gw_utils::{compression::StreamDecoder, liveness::Liveness};
use opentelemetry::trace::{SpanContext, SpanId, TraceContextExt, TraceFlags, TraceId, TraceState};
use prometheus_client::metrics::gauge::Gauge;
use tentacle::{
    builder::MetaBuilder,
    service::{ProtocolMeta, ServiceAsyncControl},
    SessionId, SubstreamReadPart,
};
use tokio::{sync::Mutex, task::block_in_place};
use tracing::{info_span, Instrument};
use tracing_opentelemetry::OpenTelemetrySpanExt;

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
    pub p2p_stream_inbox: Arc<std::sync::Mutex<Option<P2PStream>>>,
    pub completed_initial_syncing: bool,
    pub liveness: Arc<Liveness>,
    pub buffer_len: Gauge,
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
    fn liveness(&self) -> &Liveness {
        &self.liveness
    }
}

impl BlockSyncClient {
    pub async fn run(mut self) {
        gw_metrics::REGISTRY.write().unwrap().register(
            "sync_buffer_len",
            "Number of messages in the block sync receive buffer",
            Box::new(self.buffer_len.clone()),
        );
        let mut p2p_stream = None;
        loop {
            if let Some(ref mut s) = p2p_stream {
                if let Err(err) = run_with_p2p_stream(&mut self, s).await {
                    if err.is::<gw_db::error::Error>() {
                        // Cannot recover from db error.
                        log::error!("db error, exiting: {:#}", err);
                        return;
                    }
                    if !err.is::<RecoverableCtx>() {
                        let _ = s.disconnect().await;
                        p2p_stream = None;
                    }
                    log::warn!("{:#}", err);
                }
                // TODO: backoff.
                tokio::time::sleep(Duration::from_secs(3)).await;
            } else {
                p2p_stream = self.p2p_stream_inbox.lock().unwrap().take();
                if p2p_stream.is_some() {
                    continue;
                }
                if let Err(err) = run_once_without_p2p_stream(&mut self).await {
                    if err.is::<gw_db::error::Error>() {
                        // Cannot recover from db error.
                        log::error!("db error, exiting: {:#}", err);
                        return;
                    }
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
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let mut stream = stream.take_receiver();
    // Receive from the stream promptly but only send to tx when the previous
    // one has been applied.
    //
    // When there are too many messages in the buffer that haven't been applied,
    // we skip transactions and mem block messages till next block.
    let buffer_len = client.buffer_len.clone();
    buffer_len.set(0);
    let recv_handle = tokio::spawn(async move {
        let mut buffer: VecDeque<BlockSync> = VecDeque::new();
        let mut stream_ended = false;
        loop {
            tokio::select! {
                biased;
                recv_result = stream.recv(), if !stream_ended && buffer.len() < 1024 => {
                    if let Some(msg) = recv_result? {
                        BlockSyncReader::from_slice(&msg[..])?;
                        buffer.push_back(BlockSync::new_unchecked(msg));
                        buffer_len.set(buffer.len() as u64);
                    } else {
                        stream_ended = true;
                    }
                }
                reserve_result = tx.reserve(), if !buffer.is_empty() => {
                    reserve_result?.send(buffer.pop_front().unwrap());
                    buffer_len.set(buffer.len() as u64);
                }
                // No more messages - buffer is empty and stream is ended.
                else => break,
            }
            if buffer.len() >= 512
                && matches!(
                    buffer[buffer.len() - 1].to_enum(),
                    BlockSyncUnion::LocalBlock(_)
                )
            {
                log::warn!("receive buffer too large, skipping transactions and mem blocks");
                #[allow(clippy::match_like_matches_macro)]
                buffer.retain(|msg| match msg.to_enum() {
                    BlockSyncUnion::PushTransaction(_) => false,
                    BlockSyncUnion::NextMemBlock(_) => false,
                    _ => true,
                });
                log::info!("receive buffer: {}", buffer.len());
                buffer_len.set(buffer.len() as u64);
            }
        }
        anyhow::Ok(())
    });
    while let Some(msg) = rx.recv().await {
        apply_msg(client, msg).await?;
    }
    recv_handle.await??;
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
            // Use remote span context as parent.
            let trace_id: [u8; 16] = l.trace_id().as_slice().try_into().unwrap();
            let span_id: [u8; 8] = l.span_id().as_slice().try_into().unwrap();
            let span_cx = SpanContext::new(
                TraceId::from_bytes(trace_id),
                SpanId::from_bytes(span_id),
                TraceFlags::SAMPLED,
                true,
                TraceState::default(),
            );
            let span = info_span!("handle_local_block");
            span.set_parent(opentelemetry::Context::current().with_remote_span_context(span_cx));
            handle_local_block(client, l).instrument(span).await?;
            client.liveness.tick();
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
            client.liveness.tick();
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
            client.liveness.tick();
        }
        BlockSyncUnion::NextMemBlock(m) => {
            log::info!("received mem block {}", m.block_info().number().unpack());
            if let Some(ref mem_pool) = client.mem_pool {
                let mut mem_pool = mem_pool.lock().await;
                let result = mem_pool.refresh_mem_block(
                    m.block_info(),
                    m.withdrawals().into_iter().collect(),
                    m.deposits().unpack(),
                );
                if let Err(err) = result {
                    log::warn!("{:#}", err);
                }
            }
            client.liveness.tick();
        }
        BlockSyncUnion::PushTransaction(push_tx) => {
            // Use remote span context as parent.
            let trace_id: [u8; 16] = push_tx.trace_id().as_slice().try_into().unwrap();
            let span_id: [u8; 8] = push_tx.span_id().as_slice().try_into().unwrap();
            let span_cx = SpanContext::new(
                TraceId::from_bytes(trace_id),
                SpanId::from_bytes(span_id),
                TraceFlags::SAMPLED,
                true,
                TraceState::default(),
            );
            let span = info_span!("handle_push_transaction");
            span.set_parent(opentelemetry::Context::current().with_remote_span_context(span_cx));

            let tx = push_tx.transaction();
            log::info!("received L2Transaction 0x{}", hex::encode(tx.hash()));
            if let Some(ref mem_pool) = client.mem_pool {
                let mut mem_pool = mem_pool.lock().await;
                let _guard = span.enter();
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

async fn handle_local_block(
    client: &mut BlockSyncClient,
    l: gw_types::packed::LocalBlock,
) -> Result<(), anyhow::Error> {
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
            let store_tx = client.store.begin_transaction();
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
    Ok(())
}

pub struct P2PStream {
    id: SessionId,
    control: ServiceAsyncControl,
    read_part: Option<SubstreamReadPart>,
    decoder: StreamDecoder,
}

impl P2PStream {
    /// After calling this, you can only receive from the returned stream, and
    /// self can only be used for disconnecting. (This is for receiving from
    /// another task.)
    fn take_receiver(&mut self) -> Self {
        Self {
            id: self.id,
            control: self.control.clone(),
            read_part: self.read_part.take(),
            decoder: core::mem::take(&mut self.decoder),
        }
    }

    async fn recv(&mut self) -> Result<Option<Bytes>> {
        let receiver = self.read_part.as_mut().context("stream is taken")?;
        Ok(if let Some(msg) = receiver.try_next().await? {
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

/// The p2p protocol just sends the p2p stream to the client.
pub fn block_sync_client_protocol(
    stream_inbox: Arc<std::sync::Mutex<Option<P2PStream>>>,
) -> ProtocolMeta {
    let spawn = FnSpawn(move |context, control, read_part| {
        let control = control.clone();
        let id = context.id;
        let stream = P2PStream {
            id,
            control,
            read_part: Some(read_part),
            decoder: StreamDecoder::new(),
        };
        *stream_inbox.lock().unwrap() = Some(stream);
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
