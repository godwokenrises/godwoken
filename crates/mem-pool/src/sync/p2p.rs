use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
    time::Duration,
};

use futures::StreamExt;
use gw_p2p_network::FnSpawn;
use gw_types::{
    packed::{
        P2PSyncRequestBuilder, P2PSyncRequestReader, P2PSyncResponse, P2PSyncResponseBuilder,
        P2PSyncResponseReader, P2PSyncResponseUnion, RefreshMemBlockMessage,
        RefreshMemBlockMessageBuilder, RefreshMemBlockMessageReader, RefreshMemBlockMessageUnion,
        RefreshMemBlockMessageVec, RefreshMemBlockMessageVecBuilder, TryAgainBuilder,
    },
    prelude::{Builder, Entity, Pack, Reader, Unpack},
};
use tentacle::{
    builder::MetaBuilder,
    error::SendErrorKind,
    service::{ProtocolMeta, ServiceAsyncControl},
    ProtocolId, SessionId, SubstreamReadPart,
};
use tokio::sync::Mutex;

use crate::pool::MemPool;

const P2P_MEM_BLOCK_SYNC_PROTOCOL: ProtocolId = ProtocolId::new(1);

const KEEP_BLOCKS: u64 = 10;

/// Buffer `RefreshMemBlockMessage`s for recent blocks.
#[derive(Default)]
struct MessageBuffer {
    first_block: u64,
    current_block: u64,
    // buffer[n] contains messages for block n.
    buffer: BTreeMap<u64, Vec<RefreshMemBlockMessage>>,
}

impl MessageBuffer {
    fn push(&mut self, msg: RefreshMemBlockMessage) {
        match msg.to_enum() {
            RefreshMemBlockMessageUnion::NextMemBlock(_) => {
                // Skip first block, because we may not have all the transactions.
                if self.current_block != self.first_block {
                    self.buffer.entry(self.current_block).or_default().push(msg);
                }
            }
            RefreshMemBlockMessageUnion::NextL2Transaction(ref tx) => {
                let block = tx.mem_block_number().unpack();
                if self.current_block == 0 {
                    self.first_block = block;
                    self.current_block = block;
                    return;
                }
                // Skip first block, because we may not have all the transactions.
                if block == self.first_block {
                    return;
                }
                // Remove blocks that are too old now.
                if block > self.current_block {
                    self.current_block = block;
                    self.buffer = self
                        .buffer
                        .split_off(&self.current_block.saturating_sub(KEEP_BLOCKS - 1));
                }
                self.buffer.entry(block).or_default().push(msg);
            }
        }
    }

    fn first_block_buffered(&self) -> Option<u64> {
        self.buffer.keys().next().copied()
    }

    fn get(&self, block: u64) -> Option<RefreshMemBlockMessageVec> {
        self.buffer.get(&block).map(|messages| {
            RefreshMemBlockMessageVecBuilder::default()
                .extend(messages.iter().cloned())
                .build()
        })
    }
}

#[derive(Default)]
pub(crate) struct SyncServerState {
    subscribers: HashSet<SessionId>,
    buffer: MessageBuffer,
}

pub(crate) struct Publisher {
    control: ServiceAsyncControl,
    shared: Arc<Mutex<SyncServerState>>,
}

impl Publisher {
    pub(crate) async fn publish(
        &mut self,
        msg: RefreshMemBlockMessageUnion,
    ) -> Result<(), SendErrorKind> {
        match msg {
            RefreshMemBlockMessageUnion::NextL2Transaction(ref tx) => {
                log::info!(
                    "buffering and broadcasting tx in block {}",
                    tx.mem_block_number().unpack()
                );
            }
            RefreshMemBlockMessageUnion::NextMemBlock(ref b) => {
                log::info!(
                    "buffering and broadcasting NextMemBlock, block {}",
                    b.block_info().number().unpack(),
                );
            }
        }
        let msg = RefreshMemBlockMessageBuilder::default().set(msg).build();
        let mut shared = self.shared.lock().await;
        shared.buffer.push(msg.clone());
        for s in &shared.subscribers {
            let _ = self
                .control
                .send_message_to(*s, P2P_MEM_BLOCK_SYNC_PROTOCOL, msg.as_bytes())
                .await;
        }
        Ok(())
    }
}

pub(crate) fn sync_server_protocol(shared: Arc<Mutex<SyncServerState>>) -> ProtocolMeta {
    MetaBuilder::new()
        .id(P2P_MEM_BLOCK_SYNC_PROTOCOL)
        .protocol_spawn(FnSpawn(move |context, control, mut read_part| {
            let control = control.clone();
            let shared = shared.clone();
            tokio::spawn(async move {
                while let Some(Ok(msg)) = read_part.next().await {
                    let block = match P2PSyncRequestReader::from_slice(&msg) {
                        Err(_) => {
                            let _ = control.disconnect(context.id).await;
                            return;
                        }
                        Ok(r) => r.block_number().unpack(),
                    };
                    let mut shared = shared.lock().await;
                    if let Some(msgs) = shared.buffer.get(block) {
                        let reply = P2PSyncResponseBuilder::default()
                            .set(P2PSyncResponseUnion::RefreshMemBlockMessageVec(msgs))
                            .build();
                        let _ = control
                            .send_message_to(
                                context.id,
                                P2P_MEM_BLOCK_SYNC_PROTOCOL,
                                reply.as_bytes(),
                            )
                            .await;
                        shared.subscribers.insert(context.id);
                        break;
                    } else {
                        let try_again = TryAgainBuilder::default()
                            .block_number(shared.buffer.first_block_buffered().unwrap_or(0).pack())
                            .build();
                        let reply = P2PSyncResponseBuilder::default()
                            .set(P2PSyncResponseUnion::TryAgain(try_again))
                            .build();
                        let _ = control
                            .send_message_to(
                                context.id,
                                P2P_MEM_BLOCK_SYNC_PROTOCOL,
                                reply.as_bytes(),
                            )
                            .await;
                    }
                }
                while let Some(Ok(_)) = read_part.next().await {}
                let mut shared = shared.lock().await;
                shared.subscribers.remove(&context.id);
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

async fn sync_client_handle_msg(
    mem_pool: &Mutex<MemPool>,
    msg: RefreshMemBlockMessageUnion,
) -> anyhow::Result<()> {
    match msg {
        RefreshMemBlockMessageUnion::NextL2Transaction(tx) => {
            let block = tx.mem_block_number().unpack();
            // Wait till mem pool tip is in sync.
            let mut mem_pool = loop {
                let mem_pool = mem_pool.lock().await;
                // TODO: notify instead of polling.
                if mem_pool.current_tip().1 < block {
                    drop(mem_pool);
                    log::info!("waiting for tip update");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                } else {
                    break mem_pool;
                }
            };
            match mem_pool.append_tx(tx.tx(), block).await {
                Ok(_) => {}
                Err(e) => {
                    let msg = e.to_string();
                    if msg == "duplicated tx" {
                        log::info!("append_tx error: duplicated tx");
                    } else {
                        return Err(e);
                    }
                }
            }
        }
        RefreshMemBlockMessageUnion::NextMemBlock(next_mem_block) => {
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
    let mut try_again_block = 0;
    loop {
        let block_number = mem_pool.lock().await.current_tip().1;
        if block_number < try_again_block {
            // TODO: notify instead of polling.
            tokio::time::sleep(Duration::from_secs(3)).await;
            continue;
        }
        log::info!("requesting messages for block {}", block_number);
        let request = P2PSyncRequestBuilder::default()
            .block_number(block_number.pack())
            .build();
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
                try_again_block = try_again.block_number().unpack();
                log::info!("try again at: {}", try_again_block);
                // TODO: notify instead of polling.
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
            P2PSyncResponseUnion::RefreshMemBlockMessageVec(vec) => {
                log::info!("got messages");
                for msg in vec {
                    sync_client_handle_msg(mem_pool, msg.to_enum()).await?;
                }
                break;
            }
        }
    }

    while let Some(msg) = read_part.next().await {
        let msg = msg?;
        RefreshMemBlockMessageReader::from_slice(&msg)?;
        let msg = RefreshMemBlockMessage::new_unchecked(msg);
        sync_client_handle_msg(mem_pool, msg.to_enum()).await?;
    }
    Ok(())
}

pub(crate) fn sync_client_protocol(mem_pool: Arc<Mutex<MemPool>>) -> ProtocolMeta {
    let spawn = FnSpawn(move |context, control, read_part| {
        let mem_pool = mem_pool.clone();
        let control = control.clone();
        let session_id = context.id;
        tokio::spawn(async move {
            if let Err(e) = sync_client(&mem_pool, session_id, &control, read_part).await {
                log::warn!("sync_client error: {:?}", e);
            }
            log::info!("sync_client ended");
            let _ = control.disconnect(session_id).await;
        });
    });
    MetaBuilder::new()
        .id(P2P_MEM_BLOCK_SYNC_PROTOCOL)
        .protocol_spawn(spawn)
        .build()
}

#[cfg(test)]
mod tests {
    use gw_types::{
        packed::{
            NextL2TransactionBuilder, RefreshMemBlockMessageBuilder, RefreshMemBlockMessageUnion,
        },
        prelude::{Builder, Pack},
    };

    use crate::sync::p2p::KEEP_BLOCKS;

    use super::MessageBuffer;

    #[test]
    fn test_message_buffer() {
        let mut m = MessageBuffer::default();
        let tx = NextL2TransactionBuilder::default()
            .mem_block_number(3u64.pack())
            .build();
        let msg = RefreshMemBlockMessageUnion::NextL2Transaction(tx);
        m.push(RefreshMemBlockMessageBuilder::default().set(msg).build());
        let tx = NextL2TransactionBuilder::default()
            .mem_block_number(3u64.pack())
            .build();
        let msg = RefreshMemBlockMessageUnion::NextL2Transaction(tx);
        m.push(RefreshMemBlockMessageBuilder::default().set(msg).build());
        assert!(m.buffer.is_empty());

        let tx = NextL2TransactionBuilder::default()
            .mem_block_number(4u64.pack())
            .build();
        let msg = RefreshMemBlockMessageUnion::NextL2Transaction(tx);
        m.push(RefreshMemBlockMessageBuilder::default().set(msg).build());
        assert_eq!(m.first_block_buffered(), Some(4));

        for i in 5..(5 + KEEP_BLOCKS) {
            let tx = NextL2TransactionBuilder::default()
                .mem_block_number(i.pack())
                .build();
            let msg = RefreshMemBlockMessageUnion::NextL2Transaction(tx);
            m.push(RefreshMemBlockMessageBuilder::default().set(msg).build());
        }
        assert_eq!(m.first_block_buffered(), Some(5));
        assert_eq!(m.buffer.len(), KEEP_BLOCKS as usize);
    }
}
