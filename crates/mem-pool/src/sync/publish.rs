use gw_types::{
    offchain::DepositInfo,
    packed::{
        BlockInfo, L2Transaction, NextMemBlock, RefreshMemBlockMessageUnion, WithdrawalRequest,
    },
    prelude::{Builder, Entity, Pack, PackVec},
};
use smol::channel::{Receiver, Sender};

use super::mq::{gw_kafka, Produce};

const CHANNEL_BUFFER_SIZE: usize = 1000;
pub(crate) struct PublishMemPoolActor {
    receiver: Receiver<RefreshMemBlockMessageUnion>,
    producer: gw_kafka::Producer,
}

impl PublishMemPoolActor {
    pub(crate) fn new(
        receiver: Receiver<RefreshMemBlockMessageUnion>,
        producer: gw_kafka::Producer,
    ) -> Self {
        Self { receiver, producer }
    }

    fn handle(&mut self, msg: RefreshMemBlockMessageUnion) {
        if let Err(err) = self.producer.send(msg) {
            log::error!("[Fan out mem block] message failed: {:?}", err);
        }
    }
}

async fn publish_handle(mut actor: PublishMemPoolActor) {
    log::info!("Fanout handle is started.");
    while let Ok(msg) = actor.receiver.recv().await {
        actor.handle(msg);
    }
}

pub(crate) struct MemPoolPublishService {
    sender: Sender<RefreshMemBlockMessageUnion>,
}

impl MemPoolPublishService {
    pub(crate) fn start(producer: gw_kafka::Producer) -> Self {
        let (sender, receiver) = smol::channel::bounded(CHANNEL_BUFFER_SIZE);

        let actor = PublishMemPoolActor::new(receiver, producer);
        smol::spawn(publish_handle(actor)).detach();
        Self { sender }
    }

    pub(crate) fn new_tx(&self, tx: L2Transaction) {
        if let Err(err) = smol::block_on(
            self.sender
                .send(RefreshMemBlockMessageUnion::L2Transaction(tx)),
        ) {
            log::error!("Send new tx message with error: {:?}", err);
        }
    }

    pub(crate) fn next_mem_block(
        &self,
        withdrawals: Vec<WithdrawalRequest>,
        deposits: Vec<DepositInfo>,
        block_info: BlockInfo,
    ) {
        let next_mem_block = NextMemBlock::new_builder()
            .block_info(block_info)
            .withdrawals(withdrawals.pack())
            .deposits(deposits.pack())
            .build();
        let msg = RefreshMemBlockMessageUnion::NextMemBlock(next_mem_block);
        if let Err(err) = smol::block_on(self.sender.send(msg)) {
            log::error!("Send mem block message with error: {:?}", err);
        }
    }
}
