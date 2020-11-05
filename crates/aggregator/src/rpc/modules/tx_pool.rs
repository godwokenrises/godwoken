use crate::chain::TxPoolImpl;
use gw_generator::GetContractCode;
use gw_jsonrpc_types::layer2::L2Transaction;
use jsonrpc_core::{Error, Result};
use jsonrpc_derive::rpc;
use parking_lot::Mutex;
use std::sync::Arc;

#[rpc(server)]
pub trait TxPoolRPC {
    #[rpc(name = "send_transaction")]
    fn send_transaction(&self, tx: L2Transaction) -> Result<()>;
}

pub struct TxPoolRPCImpl<CodeStore> {
    tx_pool: Arc<Mutex<TxPoolImpl<CodeStore>>>,
}

impl<CodeStore> TxPoolRPCImpl<CodeStore> {
    pub fn new(tx_pool: Arc<Mutex<TxPoolImpl<CodeStore>>>) -> Self {
        TxPoolRPCImpl { tx_pool }
    }
}

impl<CodeStore: Send + GetContractCode + 'static> TxPoolRPC for TxPoolRPCImpl<CodeStore> {
    fn send_transaction(&self, tx: L2Transaction) -> Result<()> {
        let mut tx_pool = self.tx_pool.lock();
        tx_pool
            .push(tx.into())
            .map_err(|err| Error::invalid_params(format!("{:?}", err)))?;
        Ok(())
    }
}
