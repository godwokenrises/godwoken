use futures::future::join_all;
use tokio::sync::mpsc;

use crate::{
    plan::Account,
    tx::{TxHandler, TxMethod},
};

pub struct BatchReqMsg {
    pub(crate) accounts: Vec<(Account, usize)>,
    pub(crate) method: TxMethod,
    pub(crate) amount: u128,
}
pub struct BatchActor {
    receiver: mpsc::Receiver<BatchReqMsg>,
    tx_handler: TxHandler,
    batch_res_sender: mpsc::Sender<BatchResMsg>,
}

pub struct BatchResMsg {
    pub pk_idx_vec: Vec<usize>,
}

impl BatchActor {
    fn new(
        receiver: mpsc::Receiver<BatchReqMsg>,
        tx_handler: TxHandler,
        batch_res_sender: mpsc::Sender<BatchResMsg>,
    ) -> Self {
        Self {
            receiver,
            tx_handler,
            batch_res_sender,
        }
    }

    fn handle(&self, msg: BatchReqMsg) {
        let BatchReqMsg {
            method,
            accounts,
            amount,
        } = msg;

        let mut to = accounts.clone();
        to.rotate_right(1);
        let tx_handler = self.tx_handler.clone();
        let batch_res_sender = self.batch_res_sender.clone();
        tokio::spawn(async move {
            let futures = accounts
                .clone()
                .into_iter()
                .zip(to.into_iter())
                .into_iter()
                .map(|((from, _), (to, _))| {
                    let tx_handler = tx_handler.clone();
                    let pk_from = from.pk;
                    let from_id = from.account_id;
                    let to_id = to.account_id;
                    async move {
                        if let TxMethod::Submit = method {
                            let _ = tx_handler
                                .submit_erc20_tx(pk_from, from_id, to_id, amount)
                                .await;
                        };
                    }
                })
                .collect::<Vec<_>>();
            let _ = join_all(futures).await;
            let pk_idx_vec = accounts.into_iter().map(|(_, idx)| idx).collect();
            let msg = BatchResMsg { pk_idx_vec };
            let _ = batch_res_sender.send(msg).await;
        });
    }
}

async fn batch_handler(mut actor: BatchActor) {
    log::info!("batch handler is running now");
    while let Some(msg) = actor.receiver.recv().await {
        actor.handle(msg);
    }
}

#[derive(Clone)]
pub struct BatchHandler {
    sender: mpsc::Sender<BatchReqMsg>,
}

impl BatchHandler {
    pub fn new(tx_handler: TxHandler, batch_res_sender: mpsc::Sender<BatchResMsg>) -> Self {
        let (sender, receiver) = mpsc::channel(100);
        let actor = BatchActor::new(receiver, tx_handler, batch_res_sender);
        tokio::spawn(batch_handler(actor));
        Self { sender }
    }

    pub(crate) async fn send_batch(
        &self,
        pks: Vec<(Account, usize)>,
        method: TxMethod,
        amount: u128,
    ) {
        let msg = BatchReqMsg {
            accounts: pks,
            method,
            amount,
        };
        let _ = self.sender.send(msg).await;
    }

    #[allow(dead_code)]
    pub(crate) fn try_send_batch(
        &self,
        pks: Vec<(Account, usize)>,
        method: TxMethod,
        amount: u128,
    ) -> Result<(), Vec<(Account, usize)>> {
        let msg = BatchReqMsg {
            accounts: pks,
            method,
            amount,
        };
        self.sender.try_send(msg).map_err(|err| match err {
            mpsc::error::TrySendError::Full(msg) => {
                log::error!("send batch channel is full");
                msg.accounts
            }
            mpsc::error::TrySendError::Closed(msg) => {
                log::error!("send batch channel is closed");
                msg.accounts
            }
        })
    }
}
