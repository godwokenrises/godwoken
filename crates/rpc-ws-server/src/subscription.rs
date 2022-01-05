use ckb_channel::select;
use jsonrpc_core::{Metadata, Result};
use jsonrpc_derive::rpc;
use jsonrpc_pubsub::{
    typed::{Sink, Subscriber},
    PubSubMetadata, Session, SubscriptionId,
};
use log::error;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    RwLock,
};
use std::thread;

use crate::notify_controller::NotifyController;

use serde::{Deserialize, Serialize};

#[doc(hidden)]
pub type IoHandler = jsonrpc_pubsub::PubSubHandler<Option<SubscriptionSession>>;

/// Specifies the topic which to be added as active subscription.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Topic {
    NewErrorTxReceipt,
}

#[derive(Clone, Debug)]
pub struct SubscriptionSession {
    pub(crate) subscription_ids: Arc<RwLock<HashSet<SubscriptionId>>>,
    pub(crate) session: Arc<Session>,
}

impl SubscriptionSession {
    pub fn new(session: Session) -> Self {
        Self {
            subscription_ids: Arc::new(RwLock::new(HashSet::new())),
            session: Arc::new(session),
        }
    }
}

impl Metadata for SubscriptionSession {}

impl PubSubMetadata for SubscriptionSession {
    fn session(&self) -> Option<Arc<Session>> {
        Some(Arc::clone(&self.session))
    }
}

#[allow(clippy::needless_return)]
#[rpc(server)]
pub trait SubscriptionRpc {
    /// Context to implement the subscription RPC.
    type Metadata;

    #[pubsub(subscription = "subscribe", subscribe, name = "subscribe")]
    fn subscribe(
        &self,
        meta: Self::Metadata,
        subscriber: Subscriber<serde_json::Value>,
        topic: Topic,
    );

    #[pubsub(subscription = "subscribe", unsubscribe, name = "unsubscribe")]
    fn unsubscribe(&self, meta: Option<Self::Metadata>, id: SubscriptionId) -> Result<bool>;
}

type Subscribers = HashMap<SubscriptionId, Sink<serde_json::Value>>;

#[derive(Default)]
pub struct SubscriptionRpcImpl {
    pub(crate) id_generator: AtomicUsize,
    pub(crate) subscribers: Arc<RwLock<HashMap<Topic, Subscribers>>>,
}

impl SubscriptionRpc for SubscriptionRpcImpl {
    type Metadata = Option<SubscriptionSession>;

    fn subscribe(
        &self,
        meta: Self::Metadata,
        subscriber: Subscriber<serde_json::Value>,
        topic: Topic,
    ) {
        if let Some(session) = meta {
            let id = SubscriptionId::String(format!(
                "{:#x}",
                self.id_generator.fetch_add(1, Ordering::SeqCst)
            ));
            if let Ok(sink) = subscriber.assign_id(id.clone()) {
                let mut subscribers = self
                    .subscribers
                    .write()
                    .expect("acquiring subscribers write lock");
                subscribers
                    .entry(topic)
                    .or_default()
                    .insert(id.clone(), sink);

                session
                    .subscription_ids
                    .write()
                    .expect("acquiring subscription_ids write lock")
                    .insert(id);
            }
        }
    }

    fn unsubscribe(&self, meta: Option<Self::Metadata>, id: SubscriptionId) -> Result<bool> {
        let mut subscribers = self
            .subscribers
            .write()
            .expect("acquiring subscribers write lock");
        match meta {
            // unsubscribe handler method is explicitly called.
            Some(Some(session)) => {
                if session
                    .subscription_ids
                    .write()
                    .expect("acquiring subscription_ids write lock")
                    .remove(&id)
                {
                    Ok(subscribers.values_mut().any(|s| s.remove(&id).is_some()))
                } else {
                    Ok(false)
                }
            }
            // closed or dropped connection
            _ => {
                subscribers.values_mut().for_each(|s| {
                    s.remove(&id);
                });
                Ok(true)
            }
        }
    }
}

impl SubscriptionRpcImpl {
    pub fn new<S: ToString + std::fmt::Debug>(
        notify_controller: NotifyController,
        name: S,
    ) -> Self {
        println!("!!name: {:?}", name);
        let new_error_tx_receipt_receiver =
            notify_controller.subscribe_new_error_tx_receipt(name.to_string());

        let subscription_rpc_impl = SubscriptionRpcImpl::default();
        let subscribers = Arc::clone(&subscription_rpc_impl.subscribers);

        let thread_builder = thread::Builder::new().name(name.to_string());
        thread_builder
            .spawn(move || loop {
                select! {
                    recv(new_error_tx_receipt_receiver) -> msg => match msg {
                        Ok(error_tx_receipt) => {
                            // log::info!("received new error tx receipt: {:?}", msg);
                            let subscribers = subscribers.read().expect("acquiring subscribers read lock");
                            if let Some(new_error_tx_receipt_subscribers) = subscribers.get(&Topic::NewErrorTxReceipt) {
                                let receipt: gw_jsonrpc_types::godwoken::ErrorTxReceipt = error_tx_receipt.into();
                                let json_value = Ok(serde_json::to_value(&receipt).expect("serialization should be ok"));
                                for sink in new_error_tx_receipt_subscribers.values() {
                                    let _ = sink.notify(json_value.clone());
                                }
                            }
                        },
                        Err(err) => {
                            error!("new_error_tx_receipt closed {:?}", err);
                            break;
                        },
                    },
                }
            })
            .expect("Start SubscriptionRpc thread failed");

        subscription_rpc_impl
    }
}
