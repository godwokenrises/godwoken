use std::sync::{Arc, RwLock};
use std::{collections::HashMap, sync::Weak};

use gw_common::H256;
use gw_types::packed::{L2Transaction, WithdrawalRequestExtra};
use tracing::instrument;

use crate::registry::Request;

/// Hold in queue transactions and withdrawal requests.
///
/// (For get_transaction and get_withdrawal RPC calls.)
#[derive(Default)]
pub struct InQueueRequestMap {
    map: RwLock<HashMap<H256, Request>>,
}

impl InQueueRequestMap {
    #[instrument(skip_all, fields(hash = %faster_hex::hex_string(k.as_slice()).expect("hex_string")))]
    pub(crate) fn insert(self: &Arc<Self>, k: H256, v: Request) -> Option<InQueueRequestHandle> {
        let mut map = self.map.write().unwrap();
        let inserted = map.insert(k, v).is_none();
        if inserted {
            tracing::info!(map.len = map.len(), "inserted");
            Some(InQueueRequestHandle {
                map: Arc::downgrade(self),
                hash: k,
            })
        } else {
            None
        }
    }

    #[instrument(skip_all, fields(hash = %faster_hex::hex_string(k.as_slice()).expect("hex_string")))]
    fn remove(&self, k: &H256) {
        let mut map = self.map.write().unwrap();
        map.remove(k);
        tracing::info!(map.len = map.len(), "removed");
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
