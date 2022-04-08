use std::{collections::HashMap, sync::Arc};

use arc_swap::ArcSwapOption;
use futures::StreamExt;
use gw_common::H256;
use gw_p2p_network::{
    FnSpawn, P2P_IN_QUEUE_REQUEST_MAP_SYNC_PROTOCOL, P2P_IN_QUEUE_REQUEST_MAP_SYNC_PROTOCOL_NAME,
};
use gw_types::{
    in_queue_request_map_sync::*,
    packed::{L2Transaction, WithdrawalRequestExtra},
    prelude::*,
};
use parking_lot::RwLock;
use tentacle::{
    builder::MetaBuilder,
    service::{ProtocolMeta, ServiceAsyncControl},
};
use tracing::instrument;

use crate::registry::Request;

/// Hold in queue transactions and withdrawal requests.
///
/// (For get_transaction and get_withdrawal RPC calls.)
#[derive(Default)]
pub struct InQueueRequestMap {
    map: RwLock<HashMap<H256, Request>>,
    control: ArcSwapOption<ServiceAsyncControl>,
}

impl InQueueRequestMap {
    #[instrument(skip_all, fields(hash = %faster_hex::hex_string(k.as_slice()).expect("hex_string")))]
    pub(crate) async fn insert(&self, k: H256, v: Request) {
        {
            let mut map = self.map.write();
            map.insert(k, v.clone());
            tracing::info!(map.len = map.len(), "inserted");
        }
        if let Some(control) = self.control.load().as_deref() {
            let msg: SyncMessageUnion = match v {
                Request::Tx(tx) => tx.into(),
                Request::Withdrawal(w) => w.into(),
            };
            let msg = SyncMessage::new_builder().set(msg).build();
            let _ = control
                .filter_broadcast(
                    tentacle::service::TargetSession::All,
                    P2P_IN_QUEUE_REQUEST_MAP_SYNC_PROTOCOL,
                    msg.as_bytes(),
                )
                .await;
        }
    }

    #[instrument(skip_all, fields(hash = %faster_hex::hex_string(k.as_slice()).expect("hex_string")))]
    pub(crate) async fn remove(&self, k: &H256) {
        {
            let mut map = self.map.write();
            map.remove(k);
            tracing::info!(map.len = map.len(), "removed");
        }
        if let Some(control) = self.control.load().as_deref() {
            let req = RemoveRequest::new_builder().hash(k.pack()).build();
            let msg = SyncMessageUnion::from(req);
            let msg = SyncMessage::new_builder().set(msg).build();
            let _ = control
                .filter_broadcast(
                    tentacle::service::TargetSession::All,
                    P2P_IN_QUEUE_REQUEST_MAP_SYNC_PROTOCOL,
                    msg.as_bytes(),
                )
                .await;
        }
    }

    pub(crate) fn get_transaction(&self, k: &H256) -> Option<L2Transaction> {
        match self.map.read().get(k)? {
            Request::Tx(tx) => Some(tx.clone()),
            _ => None,
        }
    }

    pub(crate) fn get_withdrawal(&self, k: &H256) -> Option<WithdrawalRequestExtra> {
        match self.map.read().get(k)? {
            Request::Withdrawal(w) => Some(w.clone()),
            _ => None,
        }
    }

    pub fn set_p2p_control(&self, control: ServiceAsyncControl) {
        self.control.store(Some(Arc::new(control)));
    }
}

pub fn sync_server_protocol() -> ProtocolMeta {
    MetaBuilder::new()
        .name(|_| P2P_IN_QUEUE_REQUEST_MAP_SYNC_PROTOCOL_NAME.into())
        .id(P2P_IN_QUEUE_REQUEST_MAP_SYNC_PROTOCOL)
        .build()
}

pub fn sync_client_protocol(map: Arc<InQueueRequestMap>) -> ProtocolMeta {
    MetaBuilder::new()
        .name(|_| P2P_IN_QUEUE_REQUEST_MAP_SYNC_PROTOCOL_NAME.into())
        .id(P2P_IN_QUEUE_REQUEST_MAP_SYNC_PROTOCOL)
        .protocol_spawn(FnSpawn(move |_, _, mut read_part| {
            let map = map.clone();
            tokio::spawn(async move {
                // Clear the map on re-connect, otherwise requests that are
                // removed when we are not connected are leaked.
                map.map.write().clear();
                while let Some(Ok(msg)) = read_part.next().await {
                    if let Err(e) = SyncMessageReader::verify(&msg, false) {
                        tracing::warn!("failed to verify message: {:?}", e);
                        return;
                    }
                    let msg = SyncMessage::new_unchecked(msg);
                    match msg.to_enum() {
                        SyncMessageUnion::L2Transaction(tx) => {
                            map.insert(tx.hash().into(), Request::Tx(tx)).await;
                        }
                        SyncMessageUnion::WithdrawalRequestExtra(w) => {
                            map.insert(w.hash().into(), Request::Withdrawal(w)).await;
                        }
                        SyncMessageUnion::RemoveRequest(req) => {
                            map.remove(&req.hash().unpack()).await;
                        }
                    }
                }
            });
        }))
        .build()
}
