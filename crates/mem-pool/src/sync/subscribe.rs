use std::{sync::Arc, time::Duration};

use anyhow::Result;
use gw_config::SubscribeMemPoolConfig;
use gw_types::packed::*;
use gw_types::prelude::Unpack;
use tokio::sync::Mutex;

use crate::pool::MemPool;

use super::mq::{gw_kafka, Consume};

const CONSUME_LATENCY: u64 = 200;

pub(crate) struct SubscribeMemPoolService {
    mem_pool: Arc<Mutex<MemPool>>,
}

impl SubscribeMemPoolService {
    pub(crate) fn new(mem_pool: Arc<Mutex<MemPool>>) -> Self {
        Self { mem_pool }
    }

    pub(crate) async fn next_tx(&self, next: NextL2Transaction) -> Result<()> {
        let tx = next.tx();
        let block_number = next.mem_block_number().unpack();
        let tx_hash = tx.raw().hash();
        log::info!(
            "Add tx: {} from block: {} to mem block",
            hex::encode(&tx_hash),
            block_number
        );
        let mut mem_pool = self.mem_pool.lock().await;
        if let Err(err) = mem_pool.append_tx(tx, block_number).await {
            log::error!("Sync tx from full node failed: {:?}", err);
        }
        Ok(())
    }

    pub(crate) async fn next_mem_block(&self, next_mem_block: NextMemBlock) -> Result<Option<u64>> {
        log::info!(
            "Refresh next mem block: {}",
            next_mem_block.block_info().number().unpack()
        );
        let block_info = next_mem_block.block_info();
        let withdrawals = next_mem_block.withdrawals().into_iter().collect();
        let deposits = next_mem_block.deposits().unpack();

        let mut mem_pool = self.mem_pool.lock().await;
        mem_pool
            .refresh_mem_block(block_info, withdrawals, deposits)
            .await
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
    tokio::spawn(async move {
        log::info!("Spawn fan in mem_block task");
        loop {
            // This controls the latency of the consumer.
            // When some tx mutates mem state in the fullnode, the readonly node
            // will follow up after **CONSUME_LATENCY**ms at least.
            let _ = tokio::time::sleep(Duration::from_millis(CONSUME_LATENCY)).await;
            if let Err(err) = consumer.poll().await {
                log::error!("consume error: {:?}", err);
            }
        }
    });

    Ok(())
}
