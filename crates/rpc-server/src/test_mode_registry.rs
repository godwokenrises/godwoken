use anyhow::Result;
use async_trait::async_trait;
use gw_jsonrpc_types::{
    godwoken::GlobalState,
    test_mode::{ShouldProduceBlock, TestModePayload},
};
use jsonrpc_v2::{Data, MapRouter, Params, Server, Server as JsonrpcServer};

use std::sync::Arc;

type RPCServer = Arc<Server<MapRouter>>;
type BoxedRPCImpl = Box<dyn TestModeRPC + Send + Sync>;

pub const TEST_MODE_URI_PATH_PRODUCE_BLOCK: &str = "/tests/produce-block";
pub const TEST_MODE_URI_PATH_GLOBAL_STATE: &str = "/tests/global-state";

#[async_trait]
pub trait TestModeRPC {
    async fn get_global_state(&self) -> Result<GlobalState>;
    async fn next_global_state(&self, payload: TestModePayload) -> Result<()>;
    async fn should_produce_next_block(&self) -> Result<ShouldProduceBlock>;
}

pub struct TestModeRegistry {
    rpc_impl: Arc<BoxedRPCImpl>,
}

impl TestModeRegistry {
    pub fn new<T: TestModeRPC + Send + Sync + 'static>(rpc_impl: T) -> Self {
        Self {
            rpc_impl: Arc::new(Box::new(rpc_impl)),
        }
    }

    pub fn build_produce_block_rpc_server(&self) -> Result<RPCServer> {
        let mut server = JsonrpcServer::new();

        server = server
            .with_data(Data(Arc::clone(&self.rpc_impl)))
            .with_method("next_global_state", next_global_state)
            .with_method("should_produce_next_block", should_produce_next_block);

        Ok(server.finish())
    }

    pub fn build_global_state_rpc_server(&self) -> Result<RPCServer> {
        let mut server = JsonrpcServer::new();

        server = server
            .with_data(Data(Arc::clone(&self.rpc_impl)))
            .with_method("get_global_state", get_global_state);

        Ok(server.finish())
    }
}

async fn next_global_state(
    Params((payload,)): Params<(TestModePayload,)>,
    rpc_impl: Data<BoxedRPCImpl>,
) -> Result<()> {
    rpc_impl.next_global_state(payload).await
}

async fn get_global_state(rpc_impl: Data<BoxedRPCImpl>) -> Result<GlobalState> {
    rpc_impl.get_global_state().await
}

async fn should_produce_next_block(rpc_impl: Data<BoxedRPCImpl>) -> Result<ShouldProduceBlock> {
    rpc_impl.should_produce_next_block().await
}
