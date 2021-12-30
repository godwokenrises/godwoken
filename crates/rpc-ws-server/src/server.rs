use anyhow::{anyhow, Result};
use jsonrpc_pubsub::Session;

use crate::notify_controller::NotifyController;
use crate::subscription::{IoHandler, SubscriptionRpc, SubscriptionRpcImpl, SubscriptionSession};
use std::net::ToSocketAddrs;

pub async fn start_jsonrpc_ws_server(
    ws_rpc_address: &str,
    notify_controller: NotifyController,
) -> Result<()> {
    // TODO: read url from config
    let ws_listen_address = ws_rpc_address
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow!("invalid ws server address"))?;

    let io_handler = IoHandler::default();
    let is_subscrition_enabled = true;

    let subscription_rpc_impl =
        SubscriptionRpcImpl::start(notify_controller, "WsSubscription").await?;
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
    .start(&ws_listen_address)
    .expect("Start Jsonrpc WebSocket service");

    log::info!("Listen WS RPCServer on address {}", ws_listen_address);
    ws_server.wait()?;

    Ok(())
}
