use gw_generator::{
    smt::{Store, H256, SMT},
    Error, State,
};
use jsonrpc_core::Result;
use jsonrpc_derive::rpc;
use std::sync::{Arc, Mutex};

#[rpc(server)]
pub trait CallbackRPC {
    #[rpc(name = "callback")]
    fn callback(&self) -> Result<()>;
}

pub struct CallbackRPCImpl<S> {
    state: Arc<Mutex<SMT<S>>>,
}

impl<S: Store<H256> + Send + 'static> CallbackRPC for CallbackRPCImpl<S> {
    fn callback(&self) -> Result<()> {
        Ok(())
    }
}
