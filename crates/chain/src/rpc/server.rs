use crate::chain::TxPoolImpl;
use crate::rpc::modules::callback::{CallbackRPC, CallbackRPCImpl};
use crate::rpc::modules::tx_pool::{TxPoolRPC, TxPoolRPCImpl};
use anyhow::Result;
use crossbeam_channel::Sender;
use gw_generator::GetContractCode;
use jsonrpc_core::IoHandler;
use jsonrpc_http_server::{Server as RPCServer, ServerBuilder};
use parking_lot::Mutex;
use std::sync::Arc;

pub struct Server {
    io: IoHandler,
}

impl Server {
    pub fn new() -> Self {
        let io = IoHandler::new();
        Server { io }
    }

    pub fn enable_callback(mut self, sync_tx: Sender<()>) -> Self {
        let callback_rpc = CallbackRPCImpl::new(sync_tx);
        self.io.extend_with(callback_rpc.to_delegate());
        self
    }

    pub fn enable_tx_pool<CodeStore: GetContractCode + Send + 'static>(
        mut self,
        tx_pool: Arc<Mutex<TxPoolImpl<CodeStore>>>,
    ) -> Self {
        let tx_pool_rpc = TxPoolRPCImpl::new(tx_pool);
        self.io.extend_with(tx_pool_rpc.to_delegate());
        self
    }

    pub fn start(self, addr: &str) -> Result<RPCServer> {
        let server = ServerBuilder::new(self.io)
            .threads(2)
            .start_http(&addr.parse()?)?;

        Ok(server)
    }
}
