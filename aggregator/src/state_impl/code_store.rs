use gw_generator::syscalls::GetContractCode;
use gw_types::bytes::Bytes;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

pub struct SyncCodeStore(Arc<Mutex<HashMap<[u8; 32], Bytes>>>);

impl SyncCodeStore {
    pub fn new(inner: HashMap<[u8; 32], Bytes>) -> Self {
        SyncCodeStore(Arc::new(Mutex::new(inner)))
    }
}

impl Clone for SyncCodeStore {
    fn clone(&self) -> Self {
        SyncCodeStore(Arc::clone(&self.0))
    }
}

impl GetContractCode for SyncCodeStore {
    fn get_contract_code(&self, code_hash: &[u8; 32]) -> Option<Bytes> {
        self.0.lock().get(code_hash).cloned()
    }
}
