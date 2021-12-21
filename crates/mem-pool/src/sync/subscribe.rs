use std::{sync::Arc, time::Duration};

use anyhow::Result;
use gw_config::SubscribeMemPoolConfig;
use gw_types::packed::*;
use gw_types::prelude::Unpack;
use smol::lock::Mutex;

use crate::pool::MemPool;

use super::mq::{gw_kafka, Consume};

pub(crate) struct SubscribeMemPoolService {
    mem_pool: Arc<Mutex<MemPool>>,
}

impl SubscribeMemPoolService {
    pub(crate) fn new(mem_pool: Arc<Mutex<MemPool>>) -> Self {
        Self { mem_pool }
    }

    pub(crate) fn add_tx(&self, tx: L2Transaction) -> Result<()> {
        let tx_hash = tx.raw().hash();
        log::info!("Add tx: {} to mem block", hex::encode(&tx_hash));
        let mut mem_pool = smol::block_on(self.mem_pool.lock());
        if let Err(err) = mem_pool.append_tx(tx) {
            log::error!("Sync tx from full node failed: {:?}", err);
        }
        Ok(())
    }

    pub(crate) fn next_mem_block(&self, next_mem_block: NextMemBlock) -> Result<Option<u64>> {
        log::info!("Refresh next mem block");
        let block_info = next_mem_block.block_info();
        let withdrawals = next_mem_block.withdrawals().into_iter().collect();
        let deposits = next_mem_block.deposits().unpack();

        let mut mem_pool = smol::block_on(self.mem_pool.lock());
        mem_pool.refresh_mem_block(block_info, withdrawals, deposits)
    }
}

pub fn spawn_sub_mem_pool_task(
    mem_pool: Arc<Mutex<MemPool>>,
    mem_block_config: SubscribeMemPoolConfig,
) -> Result<()> {
    let fan_in = SubscribeMemPoolService::new(mem_pool);
    let SubscribeMemPoolConfig {
        hosts,
        topic,
        group,
    } = mem_block_config;
    let mut consumer = gw_kafka::Consumer::start(hosts, topic, group, fan_in)?;
    smol::spawn(async move {
        log::info!("Spawn fan in mem_block task");
        loop {
            let _ = smol::Timer::after(Duration::from_secs(5)).await;
            if let Err(err) = consumer.poll() {
                log::error!("consume error: {:?}", err);
            }
        }
    })
    .detach();

    Ok(())
}
