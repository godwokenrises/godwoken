//! L1 and P2P block sync.

use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result};
use bytes::Bytes;
use ckb_types::prelude::{Builder, Entity, Reader};
use futures::TryStreamExt;
use gw_chain::chain::Chain;
use gw_mem_pool::pool::MemPool;
use gw_p2p_network::{FnSpawn, P2P_BLOCK_SYNC_PROTOCOL, P2P_BLOCK_SYNC_PROTOCOL_NAME};
use gw_rpc_client::rpc_client::RPCClient;
use gw_store::{traits::chain_store::ChainStore, Store};
use gw_types::{
    packed::{
        BlockSync, BlockSyncReader, BlockSyncUnion, P2PBlockSyncResponseReader,
        P2PBlockSyncResponseUnionReader, P2PSyncRequest, Script,
    },
    prelude::Unpack,
};
use tentacle::{
    builder::MetaBuilder,
    service::{ProtocolMeta, ServiceAsyncControl},
    SessionId, SubstreamReadPart,
};
use tokio::sync::{
    mpsc::{UnboundedReceiver, UnboundedSender},
    Mutex,
};

use crate::{
    chain_updater::ChainUpdater,
    sync_l1::{revert, sync_l1, SyncL1Context},
};

pub struct BlockSyncClient {
    store: Store,
    rpc_client: RPCClient,
    chain: Arc<Mutex<Chain>>,
    mem_pool: Arc<Mutex<MemPool>>,
    chain_updater: ChainUpdater,
    rollup_type_script: Script,
    p2p_stream_receiver: UnboundedReceiver<P2PStream>,
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

pub async fn run(mut client: BlockSyncClient) -> Result<()> {
    let mut p2p_stream = None;
    loop {
        if let Some(ref mut s) = p2p_stream {
            if let Err(err) = run_with_p2p_stream(&mut client, s).await {
                if err.is::<StreamError>() {
                    // XXX: disconnect this p2p session?
                    p2p_stream = None;
                }
                log::warn!("{:#}", err);
            }
        } else {
            if let Ok(stream) = client.p2p_stream_receiver.try_recv() {
                p2p_stream = Some(stream);
                continue;
            }

            if let Err(err) = sync_l1(&client).await {
                log::warn!("{:#}", err);
            }
        }
        // TODO: backoff.
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

#[derive(Debug)]
struct StreamError;

impl std::fmt::Display for StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "stream error")
    }
}

async fn run_with_p2p_stream(client: &mut BlockSyncClient, stream: &mut P2PStream) -> Result<()> {
    loop {
        sync_l1(client).await?;
        let last_confirmed = client
            .store
            .get_last_confirmed_block_number_hash()
            .context("last confirmed")?;
        let request = P2PSyncRequest::new_builder()
            .block_hash(last_confirmed.block_hash())
            .block_number(last_confirmed.number())
            .build();
        stream.send(request.as_bytes()).await.context(StreamError)?;
        let response = stream
            .recv()
            .await
            .context(StreamError)?
            .context("unexpected end of stream")
            .context(StreamError)?;
        let response = P2PBlockSyncResponseReader::from_slice(&response).context(StreamError)?;
        match response.to_enum() {
            P2PBlockSyncResponseUnionReader::Found(_) => break,
            P2PBlockSyncResponseUnionReader::TryAgain(_) => {}
        }
    }
    while let Some(msg) = stream.recv().await.context(StreamError)? {
        BlockSyncReader::from_slice(msg.as_ref()).context(StreamError)?;
        let msg = BlockSync::new_unchecked(msg);
        apply_msg(client, msg).await?;
    }

    Ok(())
}

async fn apply_msg(client: &BlockSyncClient, msg: BlockSync) -> Result<()> {
    match msg.to_enum() {
        BlockSyncUnion::Revert(r) => {
            // TODO: check block hash.
            let store_tx = client.store.begin_transaction();
            revert(client, &store_tx, r.number_hash().number().unpack()).await?;
            store_tx.commit()?;
        }
        BlockSyncUnion::LocalBlock(l) => {
            let mut chain = client.chain.lock().await;
            let store_tx = client.store.begin_transaction();
            chain
                .update_local(
                    &store_tx,
                    l.block(),
                    l.deposit_info_vec(),
                    l.deposit_asset_scripts().into_iter().collect(),
                    l.withdrawals().into_iter().collect(),
                    l.post_global_state(),
                )
                .await?;
            // TODO: finalized custodians.
            store_tx.commit()?;
        }
        BlockSyncUnion::Submitted(s) => {
            // TODO: check block hash.
            let store_tx = client.store.begin_transaction();
            store_tx.set_block_submit_tx_hash(
                s.number_hash().number().unpack(),
                &s.tx_hash().unpack(),
            )?;
            store_tx.commit()?;
        }
        BlockSyncUnion::Confirmed(c) => {
            // TODO: check block hash.
            let store_tx = client.store.begin_transaction();
            store_tx.set_last_confirmed_block_number_hash(&c.number_hash().as_reader())?;
            store_tx.commit()?;
        }
    }
    Ok(())
}

pub struct P2PStream {
    id: SessionId,
    control: ServiceAsyncControl,
    read_part: SubstreamReadPart,
}

impl P2PStream {
    async fn recv(&mut self) -> Result<Option<Bytes>> {
        let x = self.read_part.try_next().await?;
        Ok(x)
    }

    async fn send(&mut self, msg: Bytes) -> Result<()> {
        self.control
            .send_message_to(self.id, P2P_BLOCK_SYNC_PROTOCOL, msg)
            .await?;
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
        };
        let _ = stream_tx.send(stream);
    });
    MetaBuilder::new()
        .name(|_| P2P_BLOCK_SYNC_PROTOCOL_NAME.into())
        .id(P2P_BLOCK_SYNC_PROTOCOL)
        .protocol_spawn(spawn)
        .build()
}
