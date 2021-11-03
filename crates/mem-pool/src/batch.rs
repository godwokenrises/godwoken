use anyhow::Result;
use gw_types::offchain::RunResult;
use gw_types::packed::{BlockInfo, L2Transaction, WithdrawalRequest};
use smol::channel::{Receiver, Sender, TryRecvError, TrySendError};
use smol::lock::Mutex;

use crate::constants::{MAX_BATCH_CHANNEL_BUFFER_SIZE, MAX_BATCH_TX_WITHDRAWAL_SIZE};
use crate::pool::{Inner, MemPool};

use std::sync::Arc;

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
        let (tx, rx) = smol::channel::bounded(MAX_BATCH_CHANNEL_BUFFER_SIZE);
        let background_batch = BatchTxWithdrawalInBackground::new(mem_pool, rx);
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
}

impl BatchRequest {
    fn hash(&self) -> [u8; 32] {
        match self {
            BatchRequest::Transaction(ref tx) => tx.hash(),
            BatchRequest::Withdrawal(ref w) => w.hash(),
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            BatchRequest::Transaction(_) => "tx",
            BatchRequest::Withdrawal(_) => "withdrawal",
        }
    }
}

struct BatchTxWithdrawalInBackground {
    mem_pool: Arc<Mutex<MemPool>>,
    batch_rx: Receiver<BatchRequest>,
}

// TODO: tx priority than withdrawal
impl BatchTxWithdrawalInBackground {
    fn new(mem_pool: Arc<Mutex<MemPool>>, batch_rx: Receiver<BatchRequest>) -> Self {
        BatchTxWithdrawalInBackground { mem_pool, batch_rx }
    }

    async fn run(self) {
        let mut batch = Vec::with_capacity(MAX_BATCH_TX_WITHDRAWAL_SIZE);

        loop {
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
            while batch.len() < MAX_BATCH_TX_WITHDRAWAL_SIZE {
                match self.batch_rx.try_recv() {
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
                for req in batch.drain(..) {
                    let req_hash = req.hash();
                    let req_kind = req.kind();

                    if let Err(err) = match req {
                        BatchRequest::Transaction(tx) => mem_pool.push_transaction(tx),
                        BatchRequest::Withdrawal(w) => mem_pool.push_withdrawal_request(w),
                    } {
                        log::info!(
                            "[mem-pool packager] fail to push {} {:?} into mem-pool, err: {}",
                            req_kind,
                            faster_hex::hex_string(&req_hash),
                            err
                        )
                    }
                }
            }
        }
    }
}
