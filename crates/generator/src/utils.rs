use std::convert::TryInto;

use gw_common::{builtins::CKB_SUDT_ACCOUNT_ID, state::State};
use gw_config::BackendType;
use gw_traits::CodeStore;
use gw_types::{
    bytes::Bytes,
    core::{AllowedContractType, ScriptHashType},
    offchain::RollupContext,
    packed::{CellOutput, RawL2Transaction, Script, WithdrawalRequestExtra},
    prelude::*,
};

use crate::{
    backend_manage::BackendManage,
    error::{TransactionError, WithdrawalError},
};

pub fn get_tx_type<S: State + CodeStore>(
    rollup_context: &RollupContext,
    state: &S,
    raw_tx: &RawL2Transaction,
) -> Result<AllowedContractType, TransactionError> {
    let to_id: u32 = raw_tx.to_id().unpack();
    let receiver_script_hash = state.get_script_hash(to_id)?;
    let receiver_script = state
        .get_script(&receiver_script_hash)
        .ok_or(TransactionError::ScriptHashNotFound)?;
    rollup_context
        .rollup_config
        .allowed_contract_type_hashes()
        .into_iter()
        .find(|type_hash| type_hash.hash() == receiver_script.code_hash())
        .map(|type_hash| {
            let type_: u8 = type_hash.type_().into();
            type_.try_into().unwrap_or(AllowedContractType::Unknown)
        })
        .ok_or(TransactionError::BackendNotFound {
            script_hash: receiver_script_hash,
        })
}

pub fn verify_withdrawal_capacity(
    req: &WithdrawalRequestExtra,
    opt_sudt_script: Option<Script>,
) -> Result<(), WithdrawalError> {
    let withdrawal_capacity: u64 = req.raw().capacity().unpack();

    let (type_, data) = match opt_sudt_script {
        Some(type_) => (Some(type_).pack(), req.raw().amount().as_bytes()),
        None => (None::<Script>.pack(), Bytes::new()),
    };

    let output = CellOutput::new_builder()
        .type_(type_)
        .lock(req.owner_lock())
        .build();

    match output.occupied_capacity(data.len()) {
        Ok(min_capacity) if min_capacity > withdrawal_capacity => {
            Err(WithdrawalError::InsufficientCapacity {
                expected: min_capacity as u128,
                actual: withdrawal_capacity,
            })
        }
        Err(err) => {
            tracing::warn!(error = %err, "calculate user withdrawal cell capacity"); // Overflow
            Err(WithdrawalError::InsufficientCapacity {
                expected: u64::MAX as u128 + 1,
                actual: withdrawal_capacity,
            })
        }
        _ => Ok(()),
    }
}

pub fn get_polyjuice_creator_id<S: State + CodeStore>(
    rollup_context: &RollupContext,
    backend_manage: &BackendManage,
    state: &S,
) -> Result<Option<u32>, gw_common::error::Error> {
    let polyjuice_backend =
        backend_manage
            .get_backends_at_height(u64::MAX)
            .and_then(|(_, backends)| {
                backends
                    .values()
                    .find(|backend| backend.backend_type == BackendType::Polyjuice)
            });
    if let Some(backend) = polyjuice_backend {
        let mut args = rollup_context.rollup_script_hash.as_slice().to_vec();
        args.extend_from_slice(&CKB_SUDT_ACCOUNT_ID.to_le_bytes());
        let script = Script::new_builder()
            .code_hash(backend.validator_script_type_hash.pack())
            .hash_type(ScriptHashType::Type.into())
            .args(args.pack())
            .build();
        let script_hash = script.hash().into();
        let polyjuice_creator_id = state.get_account_id_by_script_hash(&script_hash)?;
        Ok(polyjuice_creator_id)
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod test {
    use gw_common::h256_ext::H256Ext;
    use gw_common::H256;
    use gw_types::packed::{
        RawWithdrawalRequest, Script, WithdrawalRequest, WithdrawalRequestExtra,
    };
    use gw_types::prelude::{Builder, Entity, Pack};

    use super::verify_withdrawal_capacity;
    use crate::error::WithdrawalError;

    #[test]
    fn test_verify_withdrawal_capacity() {
        let sudt_script = Script::new_builder()
            .code_hash(H256::from_u32(1).pack())
            .args(vec![3; 32].pack())
            .build();
        let owner_lock = Script::new_builder()
            .code_hash(H256::from_u32(4).pack())
            .args(vec![5; 32].pack())
            .build();

        let fee = 50u128;
        let raw = RawWithdrawalRequest::new_builder()
            .nonce(1u32.pack())
            .capacity((500 * 10u64.pow(8)).pack())
            .amount(20u128.pack())
            .sudt_script_hash(sudt_script.hash().pack())
            .account_script_hash(H256::from_u32(10).pack())
            .owner_lock_hash(owner_lock.hash().pack())
            .fee(fee.pack())
            .build();
        let request = WithdrawalRequest::new_builder()
            .raw(raw.clone())
            .signature(vec![6u8; 65].pack())
            .build();
        let withdrawal = WithdrawalRequestExtra::new_builder()
            .request(request.clone())
            .owner_lock(owner_lock)
            .build();

        verify_withdrawal_capacity(&withdrawal, Some(sudt_script.clone())).expect("valid");

        // Insufficient Capacity
        let bad_raw = raw.as_builder().capacity(100.pack()).build();
        let bad_request = request.as_builder().raw(bad_raw).build();
        let bad_withdrawal = withdrawal.as_builder().request(bad_request).build();

        let err = verify_withdrawal_capacity(&bad_withdrawal, Some(sudt_script)).unwrap_err();
        let expected_err = WithdrawalError::InsufficientCapacity {
            expected: 154 * 10u128.pow(8),
            actual: 100,
        };
        assert_eq!(err, expected_err);
    }
}
