use crossbeam_channel::Sender;
use jsonrpc_core::Result;
use jsonrpc_derive::rpc;

#[rpc(server)]
pub trait CallbackRPC {
    #[rpc(name = "callback")]
    fn callback(&self) -> Result<()>;
}

pub struct CallbackRPCImpl {
    sync_tx: Sender<()>,
}

impl CallbackRPCImpl {
    pub fn new(sync_tx: Sender<()>) -> Self {
        CallbackRPCImpl { sync_tx }
    }
}

impl CallbackRPC for CallbackRPCImpl {
    fn callback(&self) -> Result<()> {
        // The previous notification is not handled yet, so we can ignore this
        if let Err(err) = self.sync_tx.try_send(()) {
            print!("ignore sync notify due to error: {:?}", err);
        }
        Ok(())
    }
}
