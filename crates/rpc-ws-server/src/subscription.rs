use gw_runtime::spawn;
use jsonrpc_core::{Metadata, Result};
use jsonrpc_derive::rpc;
use jsonrpc_pubsub::{
    typed::{Sink, Subscriber},
    PubSubMetadata, Session, SubscriptionId,
};
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    RwLock,
};

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
    pub async fn start<S: ToString + std::fmt::Debug>(
        notify_controller: NotifyController,
        name: S,
    ) -> Result<Self> {
        let err_receipt_rx = match notify_controller
            .subscribe_new_error_tx_receipt(name.to_string())
            .await
        {
            Ok(rx) => rx,
            Err(err) => {
                log::error!("[error tx receipt] subscribe {}", err);
                return Err(jsonrpc_core::Error {
                    code: jsonrpc_core::ErrorCode::InternalError,
                    message: "subscribe error tx receipt failed".to_string(),
                    data: None,
                });
            }
        };

        let subscription_rpc_impl = SubscriptionRpcImpl::default();
        let subscribers = Arc::clone(&subscription_rpc_impl.subscribers);

        spawn(async move {
            loop {
                let err_receipt = match err_receipt_rx.recv().await {
                    Ok(err_receipt) => err_receipt,
                    Err(err) => {
                        log::error!("[error tx receipt] notify service stop {}", err);
                        return;
                    }
                };
                log::trace!("[error tx receipt] new receipt: {:?}", err_receipt);

                let err_receipt: gw_jsonrpc_types::godwoken::ErrorTxReceipt =
                    (*err_receipt).clone().into();
                let json_err_receipt = match serde_json::to_value(&err_receipt) {
                    Ok(json) => json,
                    Err(err) => {
                        log::error!("[error tx receipt] serialize {:?} {}", err_receipt, err);
                        continue;
                    }
                };

                let subscribers = subscribers.read().expect("acquiring subscribers read lock");
                if let Some(subscribers) = subscribers.get(&Topic::NewErrorTxReceipt) {
                    for (subscriber, sink) in subscribers.iter() {
                        if let Err(err) = sink.notify(Ok(json_err_receipt.clone())) {
                            log::error!("[error tx receipt] sink notify {:?} {}", subscriber, err);
                        }
                    }
                }
            }
        });

        Ok(subscription_rpc_impl)
    }
}
