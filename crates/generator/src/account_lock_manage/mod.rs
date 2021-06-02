use std::collections::HashMap;

use gw_common::H256;
use gw_types::{
    bytes::Bytes,
    packed::{L2Transaction, Script, Signature},
};

#[cfg(debug_assertions)]
pub mod always_success;
pub mod secp256k1;

use crate::{error::LockAlgorithmError, RollupContext};

pub trait LockAlgorithm {
    fn recover(&self, message: H256, signature: &[u8]) -> Result<Bytes, LockAlgorithmError>;

    fn verify_withdrawal_signature(
        &self,
        lock_args: Bytes,
        signature: Signature,
        message: H256,
    ) -> Result<bool, LockAlgorithmError>;

    fn verify_tx(
        &self,
        ctx: &RollupContext,
        sender_script: Script,
        receiver_script: Script,
        tx: L2Transaction,
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
