//! Lumos client

use crate::collector::{Collector, Error, Header, TransactionInfo};
use crate::jsonrpc_types::collector::QueryParam;
pub struct Lumos;

impl Collector for Lumos {
    fn subscribe(&self, param: QueryParam, callback: String) -> Result<(), Error> {
        unimplemented!()
    }
    fn query_transactions(&self, param: QueryParam) -> Result<Vec<TransactionInfo>, Error> {
        unimplemented!()
    }
    fn get_transaction(&self, tx_hash: &[u8; 32]) -> Result<TransactionInfo, Error> {
        unimplemented!()
    }
    fn get_header(&self, block_hash: &[u8; 32]) -> Result<Option<Header>, Error> {
        unimplemented!()
    }
    fn get_header_by_number(&self, number: u64) -> Result<Option<Header>, Error> {
        unimplemented!()
    }
}
