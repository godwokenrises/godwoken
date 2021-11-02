use gw_types::packed::L2Transaction;
use smol::channel::{Receiver, TryRecvError};
use smol::lock::Mutex;

use crate::pool::MemPool;

use std::sync::Arc;

const BATCH_TXS: usize = 250;

pub struct Packager {
    mem_pool: Arc<Mutex<MemPool>>,
    tx_rx: Receiver<L2Transaction>,
}

impl Packager {
    pub fn new(mem_pool: Arc<Mutex<MemPool>>, tx_rx: Receiver<L2Transaction>) -> Self {
        Packager { mem_pool, tx_rx }
    }

    pub async fn run_in_background(self) {
        let mut batch = Vec::with_capacity(BATCH_TXS);

        loop {
            // Wait until we have tx
            match self.tx_rx.recv().await {
                Ok(tx) => batch.push(tx),
                Err(_) if self.tx_rx.is_closed() => {
                    log::error!("[mem-pool packager] channel shutdown");
                    return;
                }
                Err(_) => (),
            }

            while batch.len() < BATCH_TXS {
                match self.tx_rx.try_recv() {
                    Ok(tx) => batch.push(tx),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Closed) => {
                        log::error!("[mem-pool packager] channel shutdown");
                        return;
                    }
                }
            }

            {
                let mut mem_pool = self.mem_pool.lock().await;
                for tx in batch.drain(..) {
                    let tx_hash = tx.hash();
                    if let Err(err) = mem_pool.push_transaction(tx) {
                        log::info!(
                            "[mem-pool packager] fail to push tx {:?} into mem-pool, err: {}",
                            faster_hex::hex_string(&tx_hash),
                            err
                        )
                    }
                }
            }
        }
    }
}
