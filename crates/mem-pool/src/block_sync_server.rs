//! P2P sync server for local/submitted/confirmed Blocks.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use bytes::Bytes;
use futures::{StreamExt, TryStreamExt};
use gw_common::H256;
use gw_p2p_network::{FnSpawn, P2P_SYNC_PROTOCOL, P2P_SYNC_PROTOCOL_NAME};
use gw_types::{
    packed::{
        BlockSync, Confirmed, Found, L2Transaction, LocalBlock, NextMemBlock, P2PSyncRequest,
        P2PSyncRequestReader, P2PSyncResponse, Revert, Submitted, TryAgain,
    },
    prelude::*,
};
use gw_utils::compression::StreamEncoder;
use tentacle::{builder::MetaBuilder, service::ProtocolMeta};
use tokio::sync::broadcast::{channel, Receiver, Sender};

const KEEP_BLOCKS: u64 = 16;

#[derive(Default)]
struct BlockMessages {
    hash: H256,
    messages: Vec<BlockSync>,
}

pub struct BlockSyncServerState {
    // Block number -> block hash and messages.
    buffer: BTreeMap<u64, BlockMessages>,
    tx: Sender<BlockSync>,
}

impl Default for BlockSyncServerState {
    fn default() -> Self {
        Self::new()
    }
}

impl BlockSyncServerState {
    pub fn new() -> Self {
        let (tx, _) = channel(128);
        Self {
            buffer: Default::default(),
            tx,
        }
    }

    pub fn publish_local_block(&mut self, local_block: LocalBlock) {
        log::debug!("publish local block");
        let reader = local_block.as_reader();
        let raw = reader.block().raw();
        let number = raw.number().unpack();
        let hash = raw.hash();
        let msg = BlockSync::new_builder().set(local_block).build();
        self.buffer.insert(
            number,
            BlockMessages {
                hash: hash.into(),
                messages: vec![msg.clone()],
            },
        );
        let _ = self.tx.send(msg);
    }

    pub fn publish_submitted(&mut self, submitted: Submitted) {
        let number = submitted.as_reader().number_hash().number().unpack();
        let msg = BlockSync::new_builder().set(submitted).build();
        if let Some(msgs) = self.buffer.get_mut(&number) {
            msgs.messages.push(msg.clone());
        }
        let _ = self.tx.send(msg);
    }

    pub fn publish_confirmed(&mut self, confirmed: Confirmed) {
        let number = confirmed.number_hash().number().unpack();
        let msg = BlockSync::new_builder().set(confirmed).build();
        if let Some(msgs) = self.buffer.get_mut(&number) {
            msgs.messages.push(msg.clone());
        }
        let _ = self.tx.send(msg);
        // Remove messages for block number < number.saturating_sub(KEEP_BLOCKS).
        self.buffer = self
            .buffer
            .split_off(&(number.saturating_sub(KEEP_BLOCKS) + 1));
    }

    pub fn publish_revert(&mut self, revert: Revert) {
        let number = revert.number_hash().number().unpack();
        // Remove messages for reverted blocks.
        self.buffer.split_off(&(number + 1));
        let msg = BlockSync::new_builder().set(revert).build();
        let _ = self.tx.send(msg);
    }

    pub fn publish_transaction(&mut self, tx: L2Transaction) {
        if let Some((_, messages)) = self.buffer.iter_mut().rev().next() {
            let msg = BlockSync::new_builder().set(tx).build();
            messages.messages.push(msg.clone());
            let _ = self.tx.send(msg);
        }
    }

    pub fn publish_next_mem_block(&mut self, mem_block: NextMemBlock) {
        let number = mem_block.block_info().number().unpack();

        let msg = BlockSync::new_builder().set(mem_block).build();
        self.buffer.insert(
            number,
            BlockMessages {
                hash: [0; 32].into(),
                messages: vec![msg.clone()],
            },
        );

        let _ = self.tx.send(msg);
    }

    fn get_and_subscribe(
        &self,
        after: P2PSyncRequest,
    ) -> Result<(Vec<BlockSync>, Receiver<BlockSync>), TryAgain> {
        let number = after.block_number().unpack();
        if let Some(msgs) = self.buffer.get(&number) {
            if msgs.hash.as_slice() == after.block_hash().as_slice() {
                let msgs = self
                    .buffer
                    .range(number + 1..)
                    .flat_map(|(_, msgs)| msgs.messages.iter().cloned())
                    .collect();
                return Ok((msgs, self.tx.subscribe()));
            }
        }
        Err(TryAgain::default())
    }
}

pub fn block_sync_server_protocol(publisher: Arc<Mutex<BlockSyncServerState>>) -> ProtocolMeta {
    let spawn = FnSpawn(move |context, control, mut read_part| {
        let publisher = publisher.clone();
        let control = control.clone();
        let session_id = context.id;
        tokio::spawn(async move {
            // Compress messages.
            //
            // We keep using the same compression context in one session. This
            // way repeated content in later messages, e.g. transactions in
            // local blocks that are already published when pushed to mem pool,
            // will be compressed to just a few bytes.
            let mut encoder = StreamEncoder::new(3).expect("create StreamEncoder");
            'outer: while let Some(msg) = read_part.try_next().await? {
                P2PSyncRequestReader::from_slice(msg.as_ref())?;
                let request = P2PSyncRequest::new_unchecked(msg);
                let mut send = |x: Bytes| {
                    let compressed: Bytes = encoder.encode(&x).expect("compress").into();
                    log::debug!("compression: {} -> {}", x.len(), compressed.len());
                    control.send_message_to(session_id, P2P_SYNC_PROTOCOL, compressed)
                };
                let result = publisher.lock().unwrap().get_and_subscribe(request);
                match result {
                    Ok((msgs, mut receiver)) => {
                        let response = P2PSyncResponse::new_builder().set(Found::default()).build();
                        send(response.as_bytes()).await?;
                        for msg in msgs {
                            send(msg.as_bytes()).await?;
                        }
                        loop {
                            let result = tokio::select! {
                                // We don't expect more messages from the peer.
                                _ = read_part.next() => break 'outer,
                                result = receiver.recv() => result,
                            };
                            match result {
                                Ok(msg) => {
                                    send(msg.as_bytes()).await?;
                                }
                                Err(_) => {
                                    log::warn!(
                                        "subscription lagged, closing. session: {}",
                                        session_id
                                    );
                                    let _ = control.disconnect(session_id).await;
                                    break 'outer;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let response = P2PSyncResponse::new_builder().set(e).build();
                        send(response.as_bytes()).await?;
                    }
                }
            }
            anyhow::Ok(())
        });
    });
    MetaBuilder::new()
        .name(|_| P2P_SYNC_PROTOCOL_NAME.into())
        .id(P2P_SYNC_PROTOCOL)
        .protocol_spawn(spawn)
        .build()
}
