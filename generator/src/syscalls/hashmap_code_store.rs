use crate::bytes::Bytes;
use crate::syscalls::GetContractCode;
use std::collections::HashMap;

pub struct HashMapCodeStore(HashMap<[u8; 32], Bytes>);

impl HashMapCodeStore {
    pub fn new(inner: HashMap<[u8; 32], Bytes>) -> Self {
        HashMapCodeStore(inner)
    }
}

impl GetContractCode for HashMapCodeStore {
    fn get_contract_code(&self, code_hash: &[u8; 32]) -> Option<Bytes> {
        self.0.get(code_hash).cloned()
    }
}
