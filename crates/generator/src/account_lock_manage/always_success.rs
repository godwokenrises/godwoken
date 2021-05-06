use gw_common::H256;
use gw_types::{
    bytes::Bytes,
    packed::{L2Transaction, Script, Signature},
};

use crate::error::LockAlgorithmError;

use super::LockAlgorithm;

#[derive(Debug, Default)]
pub struct AlwaysSuccess;

/// Usage
/// register AlwaysSuccess to AccountLockManage
///
/// manage.register_lock_algorithm(code_hash, Box::new(AlwaysSuccess::default()));
impl LockAlgorithm for AlwaysSuccess {
    fn verify_withdrawal_signature(
        &self,
        _lock_args: Bytes,
        _signature: Signature,
        _message: H256,
    ) -> Result<bool, LockAlgorithmError> {
        Ok(true)
    }

    fn verify_tx(
        &self,
        _rollup_type_hash: H256,
        _sender_script: Script,
        _receiver_script: Script,
        _tx: L2Transaction,
    ) -> Result<bool, LockAlgorithmError> {
        Ok(true)
    }
}
