use gw_runtime::{block_on, spawn};
use gw_types::{
    offchain::DepositInfo,
    packed::{
        BlockInfo, L2Transaction, NextL2Transaction, NextMemBlock, RefreshMemBlockMessageUnion,
        WithdrawalRequest,
    },
    prelude::{Builder, Entity, Pack, PackVec},
};
use tokio::sync::mpsc::{Receiver, Sender};

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
    while let Some(msg) = actor.receiver.recv().await {
        actor.handle(msg);
    }
}

pub(crate) struct MemPoolPublishService {
    sender: Sender<RefreshMemBlockMessageUnion>,
}

impl MemPoolPublishService {
    pub(crate) fn start(producer: gw_kafka::Producer) -> Self {
        let (sender, receiver) = tokio::sync::mpsc::channel(CHANNEL_BUFFER_SIZE);

        let actor = PublishMemPoolActor::new(receiver, producer);
        spawn(publish_handle(actor));
        Self { sender }
    }

    pub(crate) fn new_tx(&self, tx: L2Transaction, current_tip_block_number: u64) {
        let next_tx = NextL2Transaction::new_builder()
            .tx(tx)
            .mem_block_number(current_tip_block_number.pack())
            .build();
        if let Err(err) = block_on(
            self.sender
                .send(RefreshMemBlockMessageUnion::NextL2Transaction(next_tx)),
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
        if let Err(err) = block_on(self.sender.send(msg)) {
            log::error!("Send mem block message with error: {:?}", err);
        }
    }
}
