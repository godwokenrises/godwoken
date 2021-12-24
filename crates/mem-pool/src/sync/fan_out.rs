use gw_types::{
    offchain::DepositInfo,
    packed::{
        BlockInfo, L2Transaction, NextMemBlock, RefreshMemBlockMessageUnion, WithdrawalRequest,
    },
    prelude::{Builder, Entity, Pack, PackVec},
};
use smol::channel::{Receiver, Sender};

use super::mq::{gw_kafka, Produce};

const CHANNEL_BUFFER_SIZE: usize = 500;
pub(crate) struct FanOutMemBlockActor {
    receiver: Receiver<RefreshMemBlockMessageUnion>,
    producer: gw_kafka::Producer,
}

impl FanOutMemBlockActor {
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

async fn fan_out_handle(mut actor: FanOutMemBlockActor) {
    log::info!("Fanout handle is started.");
    while let Ok(msg) = actor.receiver.recv().await {
        actor.handle(msg);
    }
}

pub(crate) struct FanOutMemBlockHandler {
    sender: Sender<RefreshMemBlockMessageUnion>,
}

impl FanOutMemBlockHandler {
    pub(crate) fn new(producer: gw_kafka::Producer) -> Self {
        let (sender, receiver) = smol::channel::bounded(CHANNEL_BUFFER_SIZE);

        let actor = FanOutMemBlockActor::new(receiver, producer);
        smol::spawn(fan_out_handle(actor)).detach();
        Self { sender }
    }

    pub(crate) fn new_tx(&self, tx: L2Transaction) {
        let _ = smol::block_on(
            self.sender
                .send(RefreshMemBlockMessageUnion::L2Transaction(tx)),
        );
    }

    pub(crate) fn next_mem_block(
        &self,
        withdrawals: Vec<WithdrawalRequest>,
        deposits: Vec<DepositInfo>,
        txs: Vec<L2Transaction>,
        block_info: BlockInfo,
    ) {
        let next_mem_block = NextMemBlock::new_builder()
            .block_info(block_info)
            .withdrawals(withdrawals.pack())
            .deposits(deposits.pack())
            .txs(txs.pack())
            .build();
        let msg = RefreshMemBlockMessageUnion::NextMemBlock(next_mem_block);
        let _ = smol::block_on(self.sender.send(msg));
    }
}
