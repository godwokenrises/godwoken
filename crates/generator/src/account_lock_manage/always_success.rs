use gw_common::H256;
use gw_types::{
    bytes::Bytes,
    packed::{L2Transaction, Script},
};

use crate::{error::LockAlgorithmError, RollupContext};

use super::LockAlgorithm;

#[derive(Debug, Default)]
pub struct AlwaysSuccess;

/// Usage
/// register AlwaysSuccess to AccountLockManage
///
/// manage.register_lock_algorithm(code_hash, Box::new(AlwaysSuccess::default()));
impl LockAlgorithm for AlwaysSuccess {
    fn recover(&self, _message: H256, _signature: &[u8]) -> Result<Bytes, LockAlgorithmError> {
        Ok(Default::default())
    }

    fn verify_message(
        &self,
        _lock_args: Bytes,
        _signature: Bytes,
        _message: H256,
    ) -> Result<bool, LockAlgorithmError> {
        Ok(true)
    }

    fn verify_tx(
        &self,
        _ctx: &RollupContext,
        _sender_script: Script,
        _receiver_script: Script,
        _tx: &L2Transaction,
    ) -> Result<bool, LockAlgorithmError> {
        Ok(true)
    }
}
