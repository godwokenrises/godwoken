use std::sync::Arc;

use anyhow::Result;
use gw_config::SyncMemBlockConfig;
use gw_types::packed::*;
use gw_types::prelude::Unpack;
use smol::lock::Mutex;

use crate::pool::MemPool;

use super::mq::{gw_kafka, Consume};

pub(crate) struct FanInMemBlock {
    mem_pool: Arc<Mutex<MemPool>>,
}

impl FanInMemBlock {
    pub(crate) fn new(mem_pool: Arc<Mutex<MemPool>>) -> Self {
        Self { mem_pool }
    }

    pub(crate) fn add_tx(&self, tx: L2Transaction) -> Result<()> {
        let mut mem_pool = smol::block_on(self.mem_pool.lock());
        if let Err(err) = mem_pool.push_transaction(tx) {
            log::error!("Sync tx from full node failed: {:?}", err);
        }
        Ok(())
    }

    pub(crate) fn next_mem_block(&self, next_mem_block: NextMemBlock) -> Result<()> {
        let block_info = next_mem_block.block_info();
        let withdrawals = next_mem_block.withdrawals().into_iter().collect();
        let deposits = next_mem_block.deposits().unpack();
        let txs = next_mem_block.txs().into_iter().collect();

        let mut mem_pool = smol::block_on(self.mem_pool.lock());
        if let Err(err) = mem_pool.refresh_mem_block(block_info, withdrawals, deposits, txs) {
            log::error!("Refresh mem block error: {:?}", err);
        }
        Ok(())
    }
}

pub fn spawn_fan_in_mem_block_task(
    mem_pool: Arc<Mutex<MemPool>>,
    sync_mem_block_config: SyncMemBlockConfig,
) -> Result<()> {
    let fan_in = FanInMemBlock::new(mem_pool);
    let SyncMemBlockConfig {
        hosts,
        topic,
        group,
    } = sync_mem_block_config;
    let mut consumer = gw_kafka::Consumer::new(hosts, topic, group, fan_in)?;
    smol::spawn(async move {
        log::info!("Spawn fan in mem_block task");
        loop {
            if let Err(err) = consumer.poll() {
                log::error!("consume error: {:?}", err);
            }
        }
    })
    .detach();

    Ok(())
}
