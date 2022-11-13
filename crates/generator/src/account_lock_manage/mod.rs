use std::{collections::HashMap, sync::Arc};

use gw_common::{registry_address::RegistryAddress, H256};
use gw_types::{
    bytes::Bytes,
    packed::{L2Transaction, Script, WithdrawalRequestExtra},
};
use gw_utils::RollupContext;

#[cfg(any(debug_assertions, feature = "enable-always-success-lock"))]
pub mod always_success;
pub mod eip712;
pub mod secp256k1;

use crate::error::LockAlgorithmError;

pub trait LockAlgorithm {
    fn recover(&self, message: H256, signature: &[u8]) -> Result<Bytes, LockAlgorithmError>;

    fn verify_tx(
        &self,
        ctx: &RollupContext,
        sender_address: RegistryAddress,
        sender_script: Script,
        receiver_script: Script,
        tx: L2Transaction,
    ) -> Result<(), LockAlgorithmError>;

    fn verify_withdrawal(
        &self,
        ctx: &RollupContext,
        sender_script: Script,
        withdrawal: &WithdrawalRequestExtra,
        withdrawal_address: RegistryAddress,
    ) -> Result<(), LockAlgorithmError>;
}

#[derive(Default, Clone)]
pub struct AccountLockManage {
    locks: HashMap<H256, Arc<dyn LockAlgorithm + Send + Sync>>,
}

impl AccountLockManage {
    pub fn register_lock_algorithm(
        &mut self,
        code_hash: H256,
        lock_algo: Arc<dyn LockAlgorithm + Send + Sync>,
    ) {
        self.locks.insert(code_hash, lock_algo);
    }

    #[allow(clippy::borrowed_box)]
    pub fn get_lock_algorithm(
        &self,
        code_hash: &H256,
    ) -> Option<&Arc<dyn LockAlgorithm + Send + Sync>> {
        self.locks.get(code_hash)
    }
}
