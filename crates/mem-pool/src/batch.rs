use anyhow::Result;
use gw_types::offchain::RunResult;
use gw_types::packed::{self, BlockInfo, L2Transaction, WithdrawalRequest};
use smol::channel::{Receiver, Sender, TryRecvError, TrySendError};
use smol::lock::Mutex;

use crate::pool::{Inner, MemPool};

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(thiserror::Error, Debug)]
pub enum BatchError {
    #[error("exceeded max batch limit")]
    ExceededMaxLimit,
    #[error("background batch service shutdown")]
    Shutdown,
    #[error("push {0}")]
    Push(anyhow::Error),
}

impl<T> From<TrySendError<T>> for BatchError {
    fn from(err: TrySendError<T>) -> Self {
        match err {
            TrySendError::Full(_) => BatchError::ExceededMaxLimit,
            TrySendError::Closed(_) => BatchError::Shutdown,
        }
    }
}

#[derive(Clone)]
pub struct MemPoolBatch {
    inner: Inner,
    background_batch_tx: Sender<BatchRequest>,
}

impl MemPoolBatch {
    pub fn new(inner: Inner, mem_pool: Arc<Mutex<MemPool>>) -> Self {
        let (tx, rx) = smol::channel::bounded(inner.config().max_batch_channel_buffer_size);
        let background_batch = BatchTxWithdrawalInBackground::new(
            mem_pool,
            rx,
            inner.config().max_batch_tx_withdrawal_size,
        );
        smol::spawn(background_batch.run()).detach();

        MemPoolBatch {
            inner,
            background_batch_tx: tx,
        }
    }

    pub fn try_push_transaction(&self, tx: L2Transaction) -> Result<(), BatchError> {
        self.background_batch_tx
            .try_send(BatchRequest::Transaction(tx))?;

        Ok(())
    }

    pub fn try_push_withdrawal_request(
        &self,
        withdrawal: WithdrawalRequest,
    ) -> Result<(), BatchError> {
        self.inner
            .verify_withdrawal_request(&withdrawal)
            .map_err(BatchError::Push)?;

        self.background_batch_tx
            .try_send(BatchRequest::Withdrawal(withdrawal))?;

        Ok(())
    }

    pub fn dump_mem_block(
        &self,
    ) -> Result<impl Future<Output = Result<packed::MemBlock>>, BatchError> {
        let (tx, rx) = smol::channel::bounded(1);
        self.background_batch_tx
            .try_send(BatchRequest::DumpMemBlock(tx))?;

        Ok(async move { Ok(rx.recv().await?) })
    }

    pub fn unchecked_execute_transaction(
        &self,
        tx: &L2Transaction,
        block_info: &BlockInfo,
    ) -> Result<RunResult> {
        self.inner.unchecked_execute_transaction(tx, block_info)
    }
}

enum BatchRequest {
    Transaction(L2Transaction),
    Withdrawal(WithdrawalRequest),
    DumpMemBlock(Sender<packed::MemBlock>),
}

impl BatchRequest {
    fn hash(&self) -> [u8; 32] {
        match self {
            BatchRequest::Transaction(ref tx) => tx.hash(),
            BatchRequest::Withdrawal(ref w) => w.hash(),
            BatchRequest::DumpMemBlock(_) => [1u8; 32],
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            BatchRequest::Transaction(_) => "tx",
            BatchRequest::Withdrawal(_) => "withdrawal",
            BatchRequest::DumpMemBlock(_) => "dump_mem_block",
        }
    }
}

struct BatchTxWithdrawalInBackground {
    mem_pool: Arc<Mutex<MemPool>>,
    batch_rx: Receiver<BatchRequest>,
    batch_size: usize,
}

// TODO: tx priority than withdrawal
impl BatchTxWithdrawalInBackground {
    fn new(
        mem_pool: Arc<Mutex<MemPool>>,
        batch_rx: Receiver<BatchRequest>,
        batch_size: usize,
    ) -> Self {
        BatchTxWithdrawalInBackground {
            mem_pool,
            batch_rx,
            batch_size,
        }
    }

    async fn run(self) {
        let mut batch = Vec::with_capacity(self.batch_size);

        loop {
            // check mem block empty slots
            loop {
                if !self.batch_rx.is_empty() {
                    let mem_pool = self.mem_pool.lock().await;
                    // continue to batch process if we have enough mem block slots
                    if !mem_pool.is_mem_txs_full(self.batch_size) {
                        break;
                    }
                }
                // sleep and try again
                smol::Timer::after(Duration::from_millis(300)).await;
            }

            // Wait until we have tx
            match self.batch_rx.recv().await {
                Ok(tx) => batch.push(tx),
                Err(_) if self.batch_rx.is_closed() => {
                    log::error!("[mem-pool packager] channel shutdown");
                    return;
                }
                Err(_) => (),
            }

            // TODO: Support interval batch
            while batch.len() < self.batch_size {
                match self.batch_rx.try_recv() {
                    Ok(tx) => batch.push(tx),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Closed) => {
                        log::error!("[mem-pool packager] channel shutdown");
                        return;
                    }
                }
            }

            let batch_size = batch.len();

            {
                let total_batch_time = Instant::now();
                let mut mem_pool = self.mem_pool.lock().await;
                log::info!(
                    "[mem-pool batch] wait {}ms to unlock mem-pool",
                    total_batch_time.elapsed().as_millis()
                );
                let db = mem_pool.inner().store().begin_transaction();
                for req in batch.drain(..) {
                    let req_hash = req.hash();
                    let req_kind = req.kind();

                    db.set_save_point();
                    if let Err(err) = match req {
                        BatchRequest::Transaction(tx) => {
                            let t = Instant::now();
                            let ret = mem_pool.push_transaction_with_db(&db, tx);
                            if ret.is_ok() {
                                log::info!(
                                    "[mem-pool batch] push tx total time {}ms",
                                    t.elapsed().as_millis()
                                );
                            }
                            ret
                        }
                        BatchRequest::Withdrawal(w) => {
                            mem_pool.push_withdrawal_request_with_db(&db, w)
                        }
                        BatchRequest::DumpMemBlock(resp_tx) => {
                            if resp_tx.is_closed() {
                                continue;
                            }
                            if let Err(err) = resp_tx.try_send(mem_pool.mem_block().pack()) {
                                log::info!("[mem-pool batch] response mem block error {}", err);
                            }
                            Ok(())
                        }
                    } {
                        db.rollback_to_save_point().expect("rollback state error");
                        log::info!(
                            "[mem-pool batch] fail to push {} {:?} into mem-pool, err: {}",
                            req_kind,
                            faster_hex::hex_string(&req_hash),
                            err
                        )
                    }
                }

                let t = Instant::now();
                if let Err(err) = db.commit() {
                    log::error!("[mem-pool batch] fail to db commit, err: {}", err);
                }
                // hold mem pool to prevent DB write confliction
                drop(mem_pool);
                log::info!(
                    "[mem-pool batch] done, batch size: {}, total time: {}ms, DB commit time: {}ms",
                    batch_size,
                    total_batch_time.elapsed().as_millis(),
                    t.elapsed().as_millis(),
                );
            }
        }
    }
}
