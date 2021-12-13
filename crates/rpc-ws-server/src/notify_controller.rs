use ckb_channel::{bounded, select, Receiver, RecvError, Sender};
use ckb_stop_handler::{SignalSender, StopHandler};
use ckb_types::core::service::Request;
use gw_types::offchain::ErrorTxReceipt;
use log::{debug, trace};
use std::collections::HashMap;
use std::thread;

pub use ckb_types::core::service::PoolTransactionEntry;

pub const SIGNAL_CHANNEL_SIZE: usize = 1;
pub const REGISTER_CHANNEL_SIZE: usize = 2;
pub const NOTIFY_CHANNEL_SIZE: usize = 128;

pub type NotifyRegister<M> = Sender<Request<String, Receiver<M>>>;

#[derive(Clone)]
pub struct NotifyController {
    stop: StopHandler<()>,

    new_error_tx_receipt_register: NotifyRegister<ErrorTxReceipt>,
    new_error_tx_receipt_notifier: Sender<ErrorTxReceipt>,
}

impl Drop for NotifyController {
    fn drop(&mut self) {
        self.stop.try_send(());
    }
}

pub struct NotifyService {
    error_tx_receipt_subscribers: HashMap<String, Sender<ErrorTxReceipt>>,
}

impl Default for NotifyService {
    fn default() -> Self {
        Self::new()
    }
}

impl NotifyService {
    pub fn new() -> Self {
        Self {
            error_tx_receipt_subscribers: HashMap::default(),
        }
    }

    /// start background single-threaded service with specified thread_name.
    pub fn start<S: ToString>(mut self, thread_name: Option<S>) -> NotifyController {
        let (signal_sender, signal_receiver) = bounded(SIGNAL_CHANNEL_SIZE);

        let (new_error_tx_receipt_registrer, new_error_tx_receipt_register_receiver) =
            bounded(NOTIFY_CHANNEL_SIZE);
        let (new_error_tx_receipt_sender, new_error_tx_receipt_receiver) =
            bounded(NOTIFY_CHANNEL_SIZE);

        let mut thread_builder = thread::Builder::new();
        if let Some(name) = thread_name {
            thread_builder = thread_builder.name(name.to_string());
        }
        let join_handle = thread_builder
            .spawn(move || loop {
                select! {
                    recv(signal_receiver) -> _ => {
                        break;
                    }

                    recv(new_error_tx_receipt_register_receiver) -> msg => self.handle_register_new_error_tx_receipt(msg),
                    recv(new_error_tx_receipt_receiver) -> msg => self.handle_notify_new_error_tx_receipt(msg),
                }
            })
            .expect("Start notify service failed");

        NotifyController {
            new_error_tx_receipt_register: new_error_tx_receipt_registrer,
            new_error_tx_receipt_notifier: new_error_tx_receipt_sender,
            stop: StopHandler::new(SignalSender::Crossbeam(signal_sender), Some(join_handle)),
        }
    }

    fn handle_register_new_error_tx_receipt(
        &mut self,
        msg: Result<Request<String, Receiver<ErrorTxReceipt>>, RecvError>,
    ) {
        match msg {
            Ok(Request {
                responder,
                arguments: name,
            }) => {
                debug!("Register new_error_tx_receipt {:?}", name);
                let (sender, receiver) = bounded(NOTIFY_CHANNEL_SIZE);
                self.error_tx_receipt_subscribers.insert(name, sender);
                let _ = responder.send(receiver);
            }
            _ => debug!("Register new_error_tx_receipt channel is closed"),
        }
    }

    fn handle_notify_new_error_tx_receipt(&mut self, msg: Result<ErrorTxReceipt, RecvError>) {
        match msg {
            Ok(error_tx_receipt) => {
                trace!("event new error_tx_receipt {:?}", error_tx_receipt);
                // notify all subscribers
                for subscriber in self.error_tx_receipt_subscribers.values() {
                    let _ = subscriber.send(error_tx_receipt.clone());
                }
            }
            _ => debug!("new error tx receipt channel is closed"),
        }
    }
}

impl NotifyController {
    pub fn subscribe_new_error_tx_receipt<S: ToString>(&self, name: S) -> Receiver<ErrorTxReceipt> {
        Request::call(&self.new_error_tx_receipt_register, name.to_string())
            .expect("Subscribe new error tx receipt should be OK")
    }

    pub fn notify_new_error_tx_receipt(&self, error_tx_receipt: ErrorTxReceipt) {
        let _ = self.new_error_tx_receipt_notifier.send(error_tx_receipt);
    }
}
