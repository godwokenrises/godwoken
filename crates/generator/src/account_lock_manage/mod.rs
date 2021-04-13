use std::collections::HashMap;

use gw_common::H256;
use gw_types::{bytes::Bytes, packed::Signature};

#[cfg(debug_assertions)]
pub mod always_success;
pub mod secp256k1;

use crate::error::LockAlgorithmError;

pub trait LockAlgorithm {
    fn verify_signature(
        &self,
        lock_args: Bytes,
        signature: Signature,
        message: H256,
    ) -> Result<bool, LockAlgorithmError>;
}

pub struct AccountLockManage {
    locks: HashMap<H256, Box<dyn LockAlgorithm + Send + Sync>>,
}

impl Default for AccountLockManage {
    fn default() -> Self {
        AccountLockManage {
            locks: Default::default(),
        }
    }
}

impl AccountLockManage {
    pub fn register_lock_algorithm(
        &mut self,
        code_hash: H256,
        lock_algo: Box<dyn LockAlgorithm + Send + Sync>,
    ) {
        self.locks.insert(code_hash, lock_algo);
    }

    #[allow(clippy::borrowed_box)]
    pub fn get_lock_algorithm(
        &self,
        code_hash: &H256,
    ) -> Option<&Box<dyn LockAlgorithm + Send + Sync>> {
        self.locks.get(code_hash)
    }
}
