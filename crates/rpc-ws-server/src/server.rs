use anyhow::Result;
use jsonrpc_pubsub::Session;
use smol::lock::Mutex;

use crate::notify_controller::{NotifyController, NotifyService};
use crate::subscription::{IoHandler, SubscriptionRpc, SubscriptionRpcImpl, SubscriptionSession};
use std::net::ToSocketAddrs;

lazy_static::lazy_static! {
    pub static ref GLOBAL_NOTIFY_CONTROLLER: Mutex<NotifyController> = Mutex::new(NotifyService::new().start(Some("NotifyService")));
}

pub async fn start_jsonrpc_ws_server() -> Result<()> {
    // TODO: read url from config
    let ws_listen_address = Some("127.0.0.1:8219".to_string());

    let io_handler = IoHandler::default();
    let is_subscrition_enabled = true;

    let notify_controller = GLOBAL_NOTIFY_CONTROLLER.lock().await;
    let ws = ws_listen_address.as_ref().map(|ws_listen_address| {
        let subscription_rpc_impl =
            SubscriptionRpcImpl::new(notify_controller.clone(), "WsSubscription");
        let mut handler = io_handler.clone();
        if is_subscrition_enabled {
            handler.extend_with(subscription_rpc_impl.to_delegate());
        }
        let ws_server = jsonrpc_ws_server::ServerBuilder::with_meta_extractor(
            handler,
            |context: &jsonrpc_ws_server::RequestContext| {
                Some(SubscriptionSession::new(Session::new(context.sender())))
            },
        )
        .start(
            &ws_listen_address
                .to_socket_addrs()
                .expect("config ws_listen_address parsed")
                .next()
                .expect("config ws_listen_address parsed"),
        )
        .expect("Start Jsonrpc WebSocket service");
        log::info!("Listen WS RPCServer on address {}", ws_listen_address);

        ws_server
    });

    if let Some(server) = ws {
        server.wait()?;
    }

    Ok(())
}
