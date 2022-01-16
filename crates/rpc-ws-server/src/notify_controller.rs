use anyhow::{anyhow, bail, Result};
use async_channel::{bounded, Receiver, Sender, TrySendError};
use futures::{FutureExt, StreamExt};
use gw_types::offchain::ErrorTxReceipt;
use std::collections::HashMap;
use std::sync::Arc;

pub const SIGNAL_CHANNEL_SIZE: usize = 1;
pub const REGISTER_CHANNEL_SIZE: usize = 2;
pub const NOTIFY_CHANNEL_SIZE: usize = 128;

pub struct Request<A, R> {
    /// One shot channel for the service to send back the response.
    pub resp_tx: Sender<R>,
    /// Request arguments.
    pub arguments: A,
}

impl<A, R> Request<A, R> {
    /// Call the service with the arguments and wait for the response.
    pub async fn call(sender: &Sender<Request<A, R>>, arguments: A) -> Result<R> {
        let (resp_tx, resp_rx) = bounded(1);
        if let Err(err) = sender.try_send(Request { resp_tx, arguments }) {
            bail!("subscribe request call {}", err);
        }

        let maybe_rx = resp_rx.recv().await;
        maybe_rx.map_err(|err| anyhow!("no response for subscribe request {}", err))
    }
}

pub type NotifyRegister<M> = Sender<Request<String, Receiver<M>>>;

#[derive(Clone)]
pub struct NotifyController {
    stop_tx: Sender<()>,

    subscribe_err_receipt_tx: NotifyRegister<Arc<ErrorTxReceipt>>,
    err_receipt_tx: Sender<Arc<ErrorTxReceipt>>,
}

pub struct NotifyService {
    error_receipt_subscribers: HashMap<String, Sender<Arc<ErrorTxReceipt>>>,
}

impl Default for NotifyService {
    fn default() -> Self {
        Self::new()
    }
}

impl NotifyService {
    pub fn new() -> Self {
        Self {
            error_receipt_subscribers: HashMap::default(),
        }
    }

    /// start background single-threaded service with specified thread_name.
    pub fn start(mut self) -> NotifyController {
        let (stop_tx, stop_rx) = bounded(SIGNAL_CHANNEL_SIZE);

        let (subscribe_err_receipt_tx, subscribe_err_receipt_rx) = bounded(NOTIFY_CHANNEL_SIZE);
        let (err_receipt_tx, err_receipt_rx) = bounded(NOTIFY_CHANNEL_SIZE);

        tokio::spawn(async move {
            let mut subscribe_err_receipt_rx = subscribe_err_receipt_rx.fuse();
            let mut err_receipt_rx = err_receipt_rx.fuse();

            loop {
                futures::select! {
                    _shutdown = stop_rx.recv().fuse() => {
                        log::info!("[error tx receipt] notify service stop");
                        return;
                    },
                    opt_request = subscribe_err_receipt_rx.next() => match opt_request {
                        Some(request) => self.handle_subscribe_err_tx_receipt(request),
                        None => {
                            log::error!("[error tx receipt] subscribe sender dropped");
                            return;
                        }
                    },
                    opt_receipt = err_receipt_rx.next() => match opt_receipt {
                        Some(err_receipt) => self.handle_notify_new_error_tx_receipt(err_receipt).await,
                        None => {
                            log::error!("[error tx receipt] receipt sender dropped");
                            return;
                        }
                    },
                    complete => {
                        log::error!("[error tx receipt] all notify service sender dropped");
                        return;
                    }
                }
            }
        });

        NotifyController {
            stop_tx,
            subscribe_err_receipt_tx,
            err_receipt_tx,
        }
    }

    fn handle_subscribe_err_tx_receipt(
        &mut self,
        req: Request<String, Receiver<Arc<ErrorTxReceipt>>>,
    ) {
        let Request {
            resp_tx,
            arguments: name,
        } = req;
        log::trace!("[error tx receipt] new subscriber {}", name);

        let (sender, receiver) = bounded(NOTIFY_CHANNEL_SIZE);
        self.error_receipt_subscribers.insert(name.clone(), sender);

        if let Err(err) = resp_tx.try_send(receiver) {
            log::warn!("[error tx receipt] resp subscribe rx to {} {}", name, err);
        }
    }

    async fn handle_notify_new_error_tx_receipt(&mut self, err_receipt: Arc<ErrorTxReceipt>) {
        log::trace!("[error tx receipt] new receipt {:?}", err_receipt);

        // notify all subscribers
        let mut closed_subscriber = vec![];
        for (name, subscriber) in self.error_receipt_subscribers.iter() {
            if let Err(err) = subscriber.send(Arc::clone(&err_receipt)).await {
                log::info!("[error tx receipt] subscriber {} closed {}", name, err);
                closed_subscriber.push(name.to_owned());
            }
        }
        for subscriber in closed_subscriber {
            self.error_receipt_subscribers.remove(&subscriber);
            log::debug!("[error tx receipt] remove subscriber {}", subscriber);
        }
    }
}

impl NotifyController {
    pub async fn subscribe_new_error_tx_receipt<S: ToString>(
        &self,
        name: S,
    ) -> Result<Receiver<Arc<ErrorTxReceipt>>> {
        Request::call(&self.subscribe_err_receipt_tx, name.to_string()).await
    }

    pub fn notify_new_error_tx_receipt(&self, error_tx_receipt: ErrorTxReceipt) {
        let err_receipt = Arc::new(error_tx_receipt);
        if let Err(err) = self.err_receipt_tx.try_send(Arc::clone(&err_receipt)) {
            match err {
                TrySendError::Closed(_) => {
                    log::error!("[error tx receipt] notify service stopped");
                }
                TrySendError::Full(_) => {
                    log::info!(
                        "[error tx receipt] notify channel is full, drop receipt {:?}",
                        err_receipt
                    );
                }
            }
        }
    }

    pub fn stop(&self) {
        if let Err(err) = self.stop_tx.try_send(()) {
            log::error!("[error tx receipt] stop notify service {}", err);
        }
    }
}
