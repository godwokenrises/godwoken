use gw_common::{registry_address::RegistryAddress, H256};
use gw_types::{
    bytes::Bytes,
    offchain::RollupContext,
    packed::{L2Transaction, Script},
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
    fn recover(&self, _message: H256, _signature: &[u8]) -> Result<Bytes, LockAlgorithmError> {
        Ok(Default::default())
    }

    fn verify_tx(
        &self,
        _ctx: &RollupContext,
        _sender_script: Script,
        _receiver_script: Script,
        _tx: L2Transaction,
    ) -> Result<(), LockAlgorithmError> {
        Ok(())
    }

    fn verify_withdrawal(
        &self,
        _sender_script: Script,
        _withdrawal: &gw_types::packed::WithdrawalRequestExtra,
        _address: RegistryAddress,
    ) -> Result<(), LockAlgorithmError> {
        Ok(())
    }
}
