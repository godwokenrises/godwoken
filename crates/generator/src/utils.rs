use std::convert::TryInto;

use anyhow::{Context, Result};
use gw_common::{builtins::CKB_SUDT_ACCOUNT_ID, state::State, H256};
use gw_config::BackendType;
use gw_traits::CodeStore;
use gw_types::{
    bytes::Bytes,
    core::{AllowedContractType, ScriptHashType},
    offchain::RollupContext,
    packed::{CellOutput, RawL2Transaction, Script, WithdrawalLockArgs, WithdrawalRequestExtra},
    prelude::*,
};

use crate::{
    backend_manage::BackendManage, error::TransactionError, generator::WithdrawalCellError,
};

pub fn get_tx_type<S: State + CodeStore>(
    rollup_context: &RollupContext,
    state: &S,
    raw_tx: &RawL2Transaction,
) -> Result<AllowedContractType> {
    let to_id: u32 = raw_tx.to_id().unpack();
    let receiver_script_hash = state.get_script_hash(to_id)?;
    let receiver_script = state
        .get_script(&receiver_script_hash)
        .ok_or_else(|| anyhow::Error::from(TransactionError::ScriptHashNotFound))
        .context("failed to get tx type")?;
    rollup_context
        .rollup_config
        .allowed_contract_type_hashes()
        .into_iter()
        .find(|type_hash| type_hash.hash() == receiver_script.code_hash())
        .map(|type_hash| {
            let type_: u8 = type_hash.type_().into();
            type_.try_into().unwrap_or(AllowedContractType::Unknown)
        })
        .ok_or_else(|| {
            anyhow::Error::from(TransactionError::BackendNotFound {
                script_hash: receiver_script_hash,
            })
        })
        .context("failed to get tx type")
}

pub fn build_withdrawal_cell_output(
    rollup_context: &RollupContext,
    req: &WithdrawalRequestExtra,
    block_hash: &H256,
    block_number: u64,
    opt_asset_script: Option<Script>,
) -> Result<(CellOutput, Bytes), WithdrawalCellError> {
    let withdrawal_capacity: u64 = req.raw().capacity().unpack();
    let lock_args: Bytes = {
        let withdrawal_lock_args = WithdrawalLockArgs::new_builder()
            .account_script_hash(req.raw().account_script_hash())
            .withdrawal_block_hash(Into::<[u8; 32]>::into(*block_hash).pack())
            .withdrawal_block_number(block_number.pack())
            .owner_lock_hash(req.raw().owner_lock_hash())
            .build();

        let mut args = Vec::new();
        args.extend_from_slice(rollup_context.rollup_script_hash.as_slice());
        args.extend_from_slice(withdrawal_lock_args.as_slice());
        let owner_lock = req.owner_lock();
        let owner_lock_hash: [u8; 32] = req.raw().owner_lock_hash().unpack();
        if owner_lock_hash != owner_lock.hash() {
            return Err(WithdrawalCellError::OwnerLock(owner_lock_hash.into()));
        }
        args.extend_from_slice(&(owner_lock.as_slice().len() as u32).to_be_bytes());
        args.extend_from_slice(owner_lock.as_slice());

        Bytes::from(args)
    };

    let lock = Script::new_builder()
        .code_hash(rollup_context.rollup_config.withdrawal_script_type_hash())
        .hash_type(ScriptHashType::Type.into())
        .args(lock_args.pack())
        .build();

    let (type_, data) = match opt_asset_script {
        Some(type_) => (Some(type_).pack(), req.raw().amount().as_bytes()),
        None => (None::<Script>.pack(), Bytes::new()),
    };

    let output = CellOutput::new_builder()
        .capacity(withdrawal_capacity.pack())
        .type_(type_)
        .lock(lock)
        .build();

    match output.occupied_capacity(data.len()) {
        Ok(min_capacity) if min_capacity > withdrawal_capacity => {
            Err(WithdrawalCellError::MinCapacity {
                min: min_capacity as u128,
                req: req.raw().capacity().unpack(),
            })
        }
        Err(err) => {
            log::debug!("calculate withdrawal capacity {}", err); // Overflow
            Err(WithdrawalCellError::MinCapacity {
                min: u64::MAX as u128 + 1,
                req: req.raw().capacity().unpack(),
            })
        }
        _ => Ok((output, data)),
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
    use gw_types::bytes::Bytes;
    use gw_types::core::ScriptHashType;
    use gw_types::offchain::RollupContext;
    use gw_types::packed::{
        RawWithdrawalRequest, RollupConfig, Script, WithdrawalRequest, WithdrawalRequestExtra,
    };
    use gw_types::prelude::{Builder, Entity, Pack, Unpack};

    use crate::generator::WithdrawalCellError;
    use crate::utils::build_withdrawal_cell_output;

    #[test]
    fn test_build_withdrawal_cell_output() {
        let rollup_context = RollupContext {
            rollup_script_hash: H256::from_u32(1),
            rollup_config: RollupConfig::new_builder()
                .withdrawal_script_type_hash(H256::from_u32(100).pack())
                .build(),
        };
        let sudt_script = Script::new_builder()
            .code_hash(H256::from_u32(1).pack())
            .args(vec![3; 32].pack())
            .build();
        let owner_lock = Script::new_builder()
            .code_hash(H256::from_u32(4).pack())
            .args(vec![5; 32].pack())
            .build();

        // ## Fulfill withdrawal request
        let req = {
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
            WithdrawalRequest::new_builder()
                .raw(raw)
                .signature(vec![6u8; 65].pack())
                .build()
        };
        let withdrawal = WithdrawalRequestExtra::new_builder()
            .request(req.clone())
            .owner_lock(owner_lock.clone())
            .build();

        let block_hash = H256::from_u32(11);
        let block_number = 11u64;
        let (output, data) = build_withdrawal_cell_output(
            &rollup_context,
            &withdrawal,
            &block_hash,
            block_number,
            Some(sudt_script.clone()),
        )
        .unwrap();

        // Basic check
        assert_eq!(output.capacity().unpack(), req.raw().capacity().unpack());
        assert_eq!(
            output.type_().as_slice(),
            Some(sudt_script.clone()).pack().as_slice()
        );
        assert_eq!(
            output.lock().code_hash(),
            rollup_context.rollup_config.withdrawal_script_type_hash()
        );
        assert_eq!(output.lock().hash_type(), ScriptHashType::Type.into());
        assert_eq!(data, req.raw().amount().as_bytes());

        // Check lock args
        let parsed_args =
            gw_utils::withdrawal::parse_lock_args(&output.lock().args().unpack()).unwrap();
        assert_eq!(
            parsed_args.rollup_type_hash.pack(),
            rollup_context.rollup_script_hash.pack()
        );
        assert_eq!(parsed_args.owner_lock.hash(), owner_lock.hash());

        let lock_args = parsed_args.lock_args;
        assert_eq!(
            lock_args.account_script_hash(),
            req.raw().account_script_hash()
        );
        assert_eq!(lock_args.withdrawal_block_hash(), block_hash.pack());
        assert_eq!(lock_args.withdrawal_block_number().unpack(), block_number);
        assert_eq!(lock_args.owner_lock_hash(), owner_lock.hash().pack());

        // ## None asset script
        let (output2, data2) = build_withdrawal_cell_output(
            &rollup_context,
            &withdrawal,
            &block_hash,
            block_number,
            None,
        )
        .unwrap();

        assert!(output2.type_().to_opt().is_none());
        assert_eq!(data2, Bytes::new());

        assert_eq!(output2.capacity().unpack(), output.capacity().unpack());
        assert_eq!(output2.lock().hash(), output.lock().hash());

        // ## Min capacity error
        let err_req = {
            let raw = req.raw().as_builder();
            let err_raw = raw
                .capacity(500u64.pack()) // ERROR: capacity not enough
                .build();
            req.clone().as_builder().raw(err_raw).build()
        };

        let err = build_withdrawal_cell_output(
            &rollup_context,
            &WithdrawalRequestExtra::new_builder()
                .request(err_req)
                .owner_lock(owner_lock)
                .build(),
            &block_hash,
            block_number,
            Some(sudt_script.clone()),
        )
        .unwrap_err();
        assert!(matches!(
            err,
            WithdrawalCellError::MinCapacity { req: _, min: _ }
        ));
        if let WithdrawalCellError::MinCapacity { req, min: _ } = err {
            assert_eq!(req, 500);
        }

        // ## Owner lock error
        let err_owner_lock = Script::new_builder()
            .code_hash([100u8; 32].pack())
            .hash_type(ScriptHashType::Data.into())
            .args(vec![99u8; 32].pack())
            .build();
        let err = build_withdrawal_cell_output(
            &rollup_context,
            &WithdrawalRequestExtra::new_builder()
                .request(req.clone())
                .owner_lock(err_owner_lock)
                .build(),
            &block_hash,
            block_number,
            Some(sudt_script),
        )
        .unwrap_err();

        assert!(matches!(err, WithdrawalCellError::OwnerLock(_)));
        if let WithdrawalCellError::OwnerLock(owner_lock_hash) = err {
            assert_eq!(req.raw().owner_lock_hash(), owner_lock_hash.pack());
        }
    }
}
