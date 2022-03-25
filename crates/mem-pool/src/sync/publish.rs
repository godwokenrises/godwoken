use gw_common::H256;
use gw_types::{
    offchain::DepositInfo,
    packed::{
        BlockInfo, L2Transaction, NextL2Transaction, NextMemBlock, RefreshMemBlockMessageUnion,
        WithdrawalRequestExtra,
    },
    prelude::{Builder, Entity, Pack, PackVec},
};
use tokio::sync::mpsc::{Receiver, Sender};

use super::{
    mq::{tokio_kafka, Produce},
    p2p,
};

const CHANNEL_BUFFER_SIZE: usize = 1000;
pub(crate) struct PublishMemPoolActor {
    receiver: Receiver<NewTipOrMessage>,
    producer: Option<tokio_kafka::Producer>,
    p2p_publisher: Option<p2p::Publisher>,
}

impl PublishMemPoolActor {
    pub(crate) fn new(
        receiver: Receiver<NewTipOrMessage>,
        producer: Option<tokio_kafka::Producer>,
        p2p_publisher: Option<p2p::Publisher>,
    ) -> Self {
        Self {
            receiver,
            producer,
            p2p_publisher,
        }
    }

    async fn handle(&mut self, msg: NewTipOrMessage) {
        match msg {
            NewTipOrMessage::Message(msg) => {
                if let Some(p) = self.p2p_publisher.as_mut() {
                    p.publish(msg.clone()).await;
                }

                if let Some(p) = self.producer.as_mut() {
                    if let Err(err) = p.send(msg).await {
                        log::error!("[Fan out mem block] message failed: {:?}", err);
                    }
                }
            }
            NewTipOrMessage::NewTip(new_tip) => {
                if let Some(p) = self.p2p_publisher.as_mut() {
                    p.handle_new_tip(new_tip).await;
                }
            }
            NewTipOrMessage::SetP2PPublisher(p2p_publisher) => {
                self.p2p_publisher = Some(p2p_publisher);
            }
        }
    }
}

async fn publish_handle(mut actor: PublishMemPoolActor) {
    log::info!("Fanout handle is started.");
    while let Some(msg) = actor.receiver.recv().await {
        actor.handle(msg).await;
    }
}

pub(crate) enum NewTipOrMessage {
    NewTip((H256, u64)),
    Message(RefreshMemBlockMessageUnion),
    SetP2PPublisher(p2p::Publisher),
}

pub(crate) struct MemPoolPublishService {
    sender: Sender<NewTipOrMessage>,
}

impl MemPoolPublishService {
    pub(crate) fn start(
        producer: Option<tokio_kafka::Producer>,
        p2p_publisher: Option<p2p::Publisher>,
    ) -> Self {
        let (sender, receiver) = tokio::sync::mpsc::channel(CHANNEL_BUFFER_SIZE);

        let actor = PublishMemPoolActor::new(receiver, producer, p2p_publisher);
        tokio::spawn(publish_handle(actor));
        Self { sender }
    }

    pub(crate) async fn new_tip(&self, new_tip: (H256, u64)) {
        if let Err(err) = self.sender.send(NewTipOrMessage::NewTip(new_tip)).await {
            log::error!("Send new tip message with error: {}", err);
        }
    }

    pub(crate) async fn new_tx(&self, tx: L2Transaction, current_tip_block_number: u64) {
        let next_tx = NextL2Transaction::new_builder()
            .tx(tx)
            .mem_block_number(current_tip_block_number.pack())
            .build();
        if let Err(err) = self
            .sender
            .send(NewTipOrMessage::Message(
                RefreshMemBlockMessageUnion::NextL2Transaction(next_tx),
            ))
            .await
        {
            log::error!("Send new tx message with error: {}", err);
        }
    }

    pub(crate) async fn next_mem_block(
        &self,
        withdrawals: Vec<WithdrawalRequestExtra>,
        deposits: Vec<DepositInfo>,
        block_info: BlockInfo,
    ) {
        let next_mem_block = NextMemBlock::new_builder()
            .block_info(block_info)
            .withdrawals(withdrawals.pack())
            .deposits(deposits.pack())
            .build();
        let msg =
            NewTipOrMessage::Message(RefreshMemBlockMessageUnion::NextMemBlock(next_mem_block));
        if let Err(err) = self.sender.send(msg).await {
            log::error!("Send mem block message with error: {}", err);
        }
    }

    pub(crate) async fn set_p2p_publisher(&self, p2p_publisher: p2p::Publisher) {
        if let Err(err) = self
            .sender
            .send(NewTipOrMessage::SetP2PPublisher(p2p_publisher))
            .await
        {
            log::error!("Send set p2p publisher error: {}", err);
        }
    }
}
