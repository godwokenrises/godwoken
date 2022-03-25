use std::{
    collections::{BTreeMap, HashSet},
    fmt,
    sync::Arc,
    time::Duration,
};

use futures::StreamExt;
use gw_common::H256;
use gw_p2p_network::{FnSpawn, P2P_MEM_BLOCK_SYNC_PROTOCOL, P2P_MEM_BLOCK_SYNC_PROTOCOL_NAME};
use gw_types::{
    packed::{
        P2PSyncMessage, P2PSyncMessageReader, P2PSyncMessageUnion, P2PSyncMessageVec,
        P2PSyncRequest, P2PSyncRequestReader, P2PSyncResponse, P2PSyncResponseReader,
        P2PSyncResponseUnion, RefreshMemBlockMessageUnion, TipSync, TryAgain,
    },
    prelude::{Builder, Entity, Pack, Reader, Unpack},
};
use tentacle::{
    builder::MetaBuilder,
    error::SendErrorKind,
    service::{ProtocolMeta, ServiceAsyncControl},
    SessionId, SubstreamReadPart,
};
use tokio::sync::{broadcast, Mutex};

use crate::pool::MemPool;

const KEEP_BLOCKS: u64 = 3;

#[derive(Default)]
struct BlockMessages {
    hash: H256,
    messages: Vec<P2PSyncMessage>,
}

/// Buffer `P2PSyncMessage`s for recent blocks.
#[derive(Default)]
struct MessageBuffer {
    // buffer[n] contains messages for block n.
    buffer: BTreeMap<u64, BlockMessages>,
}

impl MessageBuffer {
    fn push(&mut self, msg: P2PSyncMessage) {
        if let Some((_, block)) = self.buffer.iter_mut().next_back() {
            block.messages.push(msg);
        }
    }

    fn handle_new_tip(&mut self, new_tip: (H256, u64), msg: &P2PSyncMessage) {
        let later_blocks = self.buffer.split_off(&(new_tip.1 + 1));
        self.buffer
            .entry(new_tip.1)
            .and_modify(|block| {
                if block.hash != new_tip.0 || !later_blocks.is_empty() {
                    // L1 reorg.
                    block.hash = new_tip.0;
                    // Clear messages.
                    block.messages = vec![msg.clone()];
                }
            })
            .or_insert_with(|| BlockMessages {
                hash: new_tip.0,
                messages: vec![msg.clone()],
            });
        self.buffer = self
            .buffer
            .split_off(&(new_tip.1.saturating_sub(KEEP_BLOCKS - 1)));
    }

    fn first_block_buffered(&self) -> Option<(H256, u64)> {
        self.buffer.iter().next().map(|(k, v)| (v.hash, *k))
    }

    fn get_messages_after(&self, block: (H256, u64)) -> Option<P2PSyncMessageVec> {
        if self.buffer.get(&block.1).map(|b| &b.hash) != Some(&block.0) {
            return None;
        }
        let messages = self
            .buffer
            .range(block.1..)
            .map(|(_, block)| block.messages.iter())
            .flatten()
            .cloned();
        Some(P2PSyncMessageVec::new_builder().extend(messages).build())
    }
}

#[derive(Default)]
pub struct SyncServerState {
    subscribers: HashSet<SessionId>,
    buffer: MessageBuffer,
}

pub(crate) struct Publisher {
    control: ServiceAsyncControl,
    shared: Arc<Mutex<SyncServerState>>,
}

impl Publisher {
    pub(crate) async fn handle_new_tip(&mut self, new_tip: (H256, u64)) {
        let msg = P2PSyncMessage::new_builder()
            .set(P2PSyncMessageUnion::TipSync(
                TipSync::new_builder()
                    .block_number(new_tip.1.pack())
                    .block_hash(new_tip.0.pack())
                    .build(),
            ))
            .build();
        tracing::info!(tip = %HashAndNumber::from(new_tip), "publishing new tip");
        let mut shared = self.shared.lock().await;
        shared.buffer.handle_new_tip(new_tip, &msg);
        for s in &shared.subscribers {
            warn_result(
                self.control
                    .send_message_to(*s, P2P_MEM_BLOCK_SYNC_PROTOCOL, msg.as_bytes())
                    .await,
            );
        }
    }

    pub(crate) async fn publish(&mut self, msg: RefreshMemBlockMessageUnion) {
        let msg = match msg {
            RefreshMemBlockMessageUnion::NextL2Transaction(tx) => {
                let tx = tx.tx();
                tracing::info!(hash = %hex::encode(&tx.hash()), "publishing L2Transaction");
                P2PSyncMessageUnion::L2Transaction(tx)
            }
            RefreshMemBlockMessageUnion::NextMemBlock(b) => {
                tracing::info!(
                    number = b.block_info().number().unpack(),
                    "publishing NextMemBlock"
                );
                P2PSyncMessageUnion::NextMemBlock(b)
            }
        };
        let msg = P2PSyncMessage::new_builder().set(msg).build();
        let mut shared = self.shared.lock().await;
        shared.buffer.push(msg.clone());
        for s in &shared.subscribers {
            warn_result(
                self.control
                    .send_message_to(*s, P2P_MEM_BLOCK_SYNC_PROTOCOL, msg.as_bytes())
                    .await,
            );
        }
    }
}

pub fn sync_server_protocol(shared: Arc<Mutex<SyncServerState>>) -> ProtocolMeta {
    MetaBuilder::new()
        .id(P2P_MEM_BLOCK_SYNC_PROTOCOL)
        .name(|_| P2P_MEM_BLOCK_SYNC_PROTOCOL_NAME.into())
        .protocol_spawn(FnSpawn(move |context, control, mut read_part| {
            let control = control.clone();
            let shared = shared.clone();
            tokio::spawn(async move {
                let mut subscribed = false;
                while let Some(Ok(msg)) = read_part.next().await {
                    let requested_block = match P2PSyncRequestReader::from_slice(&msg) {
                        Err(_) => {
                            warn_result(control.disconnect(context.id).await);
                            return;
                        }
                        Ok(r) => (r.block_hash().unpack(), r.block_number().unpack()),
                    };
                    let mut shared = shared.lock().await;
                    if let Some(msgs) = shared.buffer.get_messages_after(requested_block) {
                        let reply = P2PSyncResponse::new_builder()
                            .set(P2PSyncResponseUnion::P2PSyncMessageVec(msgs))
                            .build();
                        warn_result(
                            control
                                .send_message_to(
                                    context.id,
                                    P2P_MEM_BLOCK_SYNC_PROTOCOL,
                                    reply.as_bytes(),
                                )
                                .await,
                        );
                        shared.subscribers.insert(context.id);
                        tracing::info!(
                            subscribers.len = shared.subscribers.len(),
                            added = context.id.value(),
                        );
                        subscribed = true;
                        break;
                    } else {
                        let try_again_block =
                            shared.buffer.first_block_buffered().unwrap_or_default();
                        drop(shared); // Unlock as soon as possible.
                        let try_again = TryAgain::new_builder()
                            .block_number(try_again_block.1.pack())
                            .block_hash(try_again_block.0.pack())
                            .build();
                        let reply = P2PSyncResponse::new_builder()
                            .set(P2PSyncResponseUnion::TryAgain(try_again))
                            .build();
                        warn_result(
                            control
                                .send_message_to(
                                    context.id,
                                    P2P_MEM_BLOCK_SYNC_PROTOCOL,
                                    reply.as_bytes(),
                                )
                                .await,
                        );
                    }
                }
                if subscribed {
                    // We are publishing and do not expect any more messages
                    // from the client.
                    //
                    // If we receive a message, or there is an error, or the
                    // stream is closed, remove the peer from subscribers and
                    // disconnect.
                    let _ = read_part.next().await;
                    warn_result(control.disconnect(context.id).await);
                    let mut shared = shared.lock().await;
                    shared.subscribers.remove(&context.id);
                    tracing::info!(
                        subscribers.len = shared.subscribers.len(),
                        removed = context.id.value(),
                    );
                }
            });
        }))
        .build()
}

pub(crate) fn sync_server_publisher(
    control: ServiceAsyncControl,
    shared: Arc<Mutex<SyncServerState>>,
) -> Publisher {
    Publisher { control, shared }
}

async fn wait_for_tip(
    mem_pool: &Mutex<MemPool>,
    tip: (H256, u64),
    at_least_once: bool,
) -> Result<(), (H256, u64)> {
    let mem_pool = mem_pool.lock().await;
    let current = mem_pool.current_tip();
    if current == tip {
        return Ok(());
    }
    if current.1 > tip.1 && !at_least_once {
        return Err(current);
    }
    let mut receiver = mem_pool.subscribe_new_tip();
    drop(mem_pool);
    tracing::info!("waiting for tip update");
    loop {
        match receiver.recv().await {
            Ok(new_tip) => {
                tracing::info!(new_tip = %HashAndNumber::from(new_tip));
                if new_tip == tip {
                    return Ok(());
                }
                if new_tip.1 > tip.1 {
                    return Err(new_tip);
                }
            }
            Err(broadcast::error::RecvError::Lagged(_)) => {}
            // This is not possible because we are holding an reference to the
            // mem_pool, which holds the sender, so the sender cannot possibly
            // be closed.
            Err(_) => panic!("tip update sender closed"),
        }
    }
}

async fn sync_client_handle_msg(
    mem_pool: &Mutex<MemPool>,
    msg: P2PSyncMessageUnion,
    current_tip: &mut (H256, u64),
) -> anyhow::Result<()> {
    match msg {
        P2PSyncMessageUnion::TipSync(tip_sync) => {
            let tip_sync: (H256, u64) = (
                tip_sync.block_hash().unpack(),
                tip_sync.block_number().unpack(),
            );
            tracing::info!(tip = %HashAndNumber::from(tip_sync), "handling tip sync");
            // Wait for at least one update if block number goes back.
            let at_least_once = tip_sync.1 < current_tip.1;
            *current_tip = tip_sync;
            if let Err(wrong_tip) = wait_for_tip(mem_pool, *current_tip, at_least_once).await {
                tracing::warn!(tip = %HashAndNumber::from(wrong_tip), "tip updated, continuing anyway");
            }
        }
        P2PSyncMessageUnion::L2Transaction(tx) => {
            tracing::info!(hash = %hex::encode(&tx.hash()), "handling L2Transaction");
            let mut mem_pool = mem_pool.lock().await;
            match mem_pool.append_tx(tx, current_tip.1).await {
                Ok(_) => {}
                Err(e) => {
                    let msg = e.to_string();
                    if msg == "duplicated tx" {
                        tracing::warn!("duplicated tx");
                    } else {
                        return Err(e);
                    }
                }
            }
        }
        P2PSyncMessageUnion::NextMemBlock(next_mem_block) => {
            tracing::info!(
                number = next_mem_block.block_info().number().unpack(),
                "handling NextMemBlock"
            );
            let block_info = next_mem_block.block_info();
            let withdrawals = next_mem_block.withdrawals().into_iter().collect();
            let deposits = next_mem_block.deposits().unpack();

            let mut mem_pool = mem_pool.lock().await;
            mem_pool
                .refresh_mem_block(block_info, withdrawals, deposits)
                .await?;
        }
    }
    Ok(())
}

async fn sync_client(
    mem_pool: &Mutex<MemPool>,
    session_id: SessionId,
    control: &ServiceAsyncControl,
    mut read_part: SubstreamReadPart,
) -> anyhow::Result<()> {
    let mut try_again_block: (H256, u64) = Default::default();
    let mut current_tip;
    loop {
        current_tip = match wait_for_tip(mem_pool, try_again_block, false).await {
            Ok(_) => try_again_block,
            Err(tip) => tip,
        };
        let request = P2PSyncRequest::new_builder()
            .block_number(current_tip.1.pack())
            .block_hash(current_tip.0.pack())
            .build();
        tracing::info!(block = %HashAndNumber::from(current_tip), "requesting");
        control
            .send_message_to(session_id, P2P_MEM_BLOCK_SYNC_PROTOCOL, request.as_bytes())
            .await?;
        let response = read_part
            .next()
            .await
            .ok_or_else(|| anyhow::format_err!("no response"))??;
        P2PSyncResponseReader::from_slice(&response)?;
        let response = P2PSyncResponse::new_unchecked(response);
        match response.to_enum() {
            P2PSyncResponseUnion::TryAgain(try_again) => {
                try_again_block = (
                    try_again.block_hash().unpack(),
                    try_again.block_number().unpack(),
                );
                tracing::info!(try_again_block = %HashAndNumber::from(try_again_block));
                if try_again_block.1 <= current_tip.1 {
                    tracing::info!("retry in 3 seconds");
                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
            }
            P2PSyncResponseUnion::P2PSyncMessageVec(vec) => {
                tracing::info!(len = vec.len(), "got messages");
                for msg in vec {
                    sync_client_handle_msg(mem_pool, msg.to_enum(), &mut current_tip).await?;
                }
                break;
            }
        }
    }

    while let Some(msg) = read_part.next().await {
        let msg = msg?;
        P2PSyncMessageReader::from_slice(&msg)?;
        let msg = P2PSyncMessage::new_unchecked(msg).to_enum();
        sync_client_handle_msg(mem_pool, msg, &mut current_tip).await?;
    }
    Ok(())
}

// Cooperate with graceful shutdown so that mem_pool can be dropped.
pub fn sync_client_protocol(
    mem_pool: Arc<Mutex<MemPool>>,
    shutdown_event: broadcast::Sender<()>,
) -> ProtocolMeta {
    let spawn = FnSpawn(move |context, control, read_part| {
        let mem_pool = mem_pool.clone();
        let control = control.clone();
        let session_id = context.id;
        let mut shutdown_event_rx = shutdown_event.subscribe();
        tokio::spawn(async move {
            let result = tokio::select! {
                _ = shutdown_event_rx.recv() => return,
                result = sync_client(&mem_pool, session_id, &control, read_part) => result,
            };
            if let Err(e) = result {
                tracing::warn!(error = %e);
            }
            tracing::info!("sync_client ended");
            warn_result(control.disconnect(session_id).await);
        });
    });
    MetaBuilder::new()
        .name(|_| P2P_MEM_BLOCK_SYNC_PROTOCOL_NAME.into())
        .id(P2P_MEM_BLOCK_SYNC_PROTOCOL)
        .protocol_spawn(spawn)
        .build()
}

// For displaying block hash and number in logging.
struct HashAndNumber {
    hash: H256,
    number: u64,
}

impl fmt::Display for HashAndNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = [0u8; 16];
        hex::encode_to_slice(&self.hash.as_slice()[..8], &mut out).expect("hex encode");
        write!(
            f,
            "number={} hash={}",
            self.number,
            std::str::from_utf8(&out).expect("from utf_8"),
        )
    }
}

impl From<(H256, u64)> for HashAndNumber {
    fn from(x: (H256, u64)) -> Self {
        Self {
            hash: x.0,
            number: x.1,
        }
    }
}

fn warn_result(result: Result<(), SendErrorKind>) {
    if let Err(e) = result {
        warn_error(e);
    }
}

#[cold]
fn warn_error(e: SendErrorKind) {
    tracing::warn!(error = ?e, "p2p network control");
}

#[cfg(test)]
mod tests {
    use gw_types::packed::P2PSyncMessage;

    use super::MessageBuffer;

    #[test]
    fn test_message_buffer() {
        let mut m = MessageBuffer::default();

        let msg = P2PSyncMessage::default();

        m.push(msg.clone());
        assert!(m.buffer.is_empty());

        m.handle_new_tip((Default::default(), 8), &msg);
        m.push(msg.clone());
        m.push(msg.clone());
        assert_eq!(m.buffer[&8].messages.len(), 3);

        m.handle_new_tip((Default::default(), 9), &msg);
        m.push(msg.clone());
        assert_eq!(m.buffer[&9].messages.len(), 2);

        assert_eq!(
            m.get_messages_after((Default::default(), 8)).unwrap().len(),
            5
        );

        assert!(m.get_messages_after(([1; 32].into(), 8)).is_none());

        m.handle_new_tip(([1; 32].into(), 9), &msg);
        m.push(msg.clone());
        assert_eq!(m.buffer[&9].messages.len(), 2);

        m.handle_new_tip(([1; 32].into(), 8), &msg);
        m.push(msg);
        assert_eq!(m.buffer[&8].messages.len(), 2);
    }
}
