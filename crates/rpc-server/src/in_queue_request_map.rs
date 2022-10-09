use std::sync::{Arc, RwLock};
use std::{collections::HashMap, sync::Weak};

use gw_common::H256;
use gw_types::packed::{L2Transaction, WithdrawalRequestExtra};
use prometheus_client::metrics::gauge::Gauge;

use crate::registry::Request;

/// Hold in queue transactions and withdrawal requests.
///
/// (For get_transaction and get_withdrawal RPC calls.)
pub struct InQueueRequestMap {
    map: RwLock<HashMap<H256, Request>>,
    queue_len: Gauge,
}

impl InQueueRequestMap {
    pub fn create_and_register_metrics() -> Self {
        let map = Self {
            map: Default::default(),
            queue_len: Default::default(),
        };
        gw_metrics::REGISTRY.write().unwrap().register(
            "in_queue_requests",
            "number of in queue requests",
            Box::new(map.queue_len.clone()),
        );
        map
    }

    pub(crate) fn insert(self: &Arc<Self>, k: H256, v: Request) -> Option<InQueueRequestHandle> {
        let mut map = self.map.write().unwrap();
        let inserted = map.insert(k, v).is_none();
        self.queue_len.set(map.len() as u64);
        if inserted {
            Some(InQueueRequestHandle {
                map: Arc::downgrade(self),
                hash: k,
            })
        } else {
            None
        }
    }

    fn remove(&self, k: &H256) {
        let mut map = self.map.write().unwrap();
        map.remove(k);
        self.queue_len.set(map.len() as u64);
    }

    pub(crate) fn get_transaction(&self, k: &H256) -> Option<L2Transaction> {
        match self.map.read().unwrap().get(k)? {
            Request::Tx(tx) => Some(tx.clone()),
            _ => None,
        }
    }

    pub(crate) fn get_withdrawal(&self, k: &H256) -> Option<WithdrawalRequestExtra> {
        match self.map.read().unwrap().get(k)? {
            Request::Withdrawal(w) => Some(w.clone()),
            _ => None,
        }
    }

    pub(crate) fn contains(&self, k: &H256) -> bool {
        self.map.read().unwrap().contains_key(k)
    }
}

/// RAII guard for the request in an InQueueRequestMap.
pub(crate) struct InQueueRequestHandle {
    map: Weak<InQueueRequestMap>,
    hash: H256,
}

impl Drop for InQueueRequestHandle {
    fn drop(&mut self) {
        if let Some(map) = self.map.upgrade() {
            map.remove(&self.hash);
        }
    }
}
