use gw_common::{builtins::CKB_SUDT_ACCOUNT_ID, ckb_decimal::CKBCapacity, state::State, H256};
use gw_traits::CodeStore;
use gw_types::{
    offchain::RollupContext,
    packed::{Script, WithdrawalRequestExtra},
    prelude::*,
    U256,
};
use tracing::instrument;

use crate::{
    constants::MAX_WITHDRAWAL_SIZE,
    error::{AccountError, WithdrawalError},
    sudt::build_l2_sudt_script,
    utils::verify_withdrawal_capacity,
    Error,
};

pub struct WithdrawalVerifier<'a, S> {
    state: &'a S,
    rollup_context: &'a RollupContext,
}

impl<'a, S: State + CodeStore> WithdrawalVerifier<'a, S> {
    pub fn new(state: &'a S, rollup_context: &'a RollupContext) -> Self {
        Self {
            state,
            rollup_context,
        }
    }

    /// Verify withdrawal request
    /// Notice this function do not perform signature check
    #[instrument(skip_all)]
    pub fn verify(
        &self,
        withdrawal: &WithdrawalRequestExtra,
        asset_script: Option<Script>,
    ) -> Result<(), Error> {
        // check withdrawal size
        if withdrawal.as_slice().len() > MAX_WITHDRAWAL_SIZE {
            return Err(WithdrawalError::ExceededMaxWithdrawalSize {
                max_size: MAX_WITHDRAWAL_SIZE,
                withdrawal_size: withdrawal.as_slice().len(),
            }
            .into());
        }

        let raw = withdrawal.request().raw();

        let account_script_hash: H256 = raw.account_script_hash().unpack();
        let sudt_script_hash: H256 = raw.sudt_script_hash().unpack();
        let amount: u128 = raw.amount().unpack();
        let capacity: u64 = raw.capacity().unpack();
        let fee = raw.fee().unpack();
        let registry_address = self
            .state
            .get_registry_address_by_script_hash(raw.registry_id().unpack(), &account_script_hash)?
            .ok_or(Error::Account(AccountError::UnknownAccount))?;

        // check capacity (use dummy block hash and number)
        verify_withdrawal_capacity(withdrawal, asset_script)?;

        // find user account
        let id = self
            .state
            .get_account_id_by_script_hash(&account_script_hash)?
            .ok_or(AccountError::UnknownAccount)?; // find Simple UDT account

        // check nonce
        let expected_nonce = self.state.get_nonce(id)?;
        let actual_nonce: u32 = raw.nonce().unpack();
        if actual_nonce != expected_nonce {
            return Err(WithdrawalError::Nonce {
                expected: expected_nonce,
                actual: actual_nonce,
            }
            .into());
        }

        // check CKB balance
        let ckb_balance = self
            .state
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &registry_address)?;
        let required_ckb_capacity = CKBCapacity::from_layer1(capacity)
            .to_layer2()
            .saturating_add(fee.into());
        if required_ckb_capacity > ckb_balance {
            return Err(WithdrawalError::Overdraft.into());
        }
        let l2_sudt_script_hash =
            build_l2_sudt_script(self.rollup_context, &sudt_script_hash).hash();
        let sudt_id = self
            .state
            .get_account_id_by_script_hash(&l2_sudt_script_hash.into())?
            .ok_or(AccountError::UnknownSUDT)?;
        // The sUDT id must not be equals to the CKB sUDT id if amount isn't 0
        if sudt_id != CKB_SUDT_ACCOUNT_ID {
            // check SUDT balance
            // user can't withdrawal 0 SUDT when non-CKB sudt_id exists
            if amount == 0 {
                return Err(WithdrawalError::NonPositiveSUDTAmount.into());
            }
            let balance = self.state.get_sudt_balance(sudt_id, &registry_address)?;
            if U256::from(amount) > balance {
                return Err(WithdrawalError::Overdraft.into());
            }
        } else if amount != 0 {
            // user can't withdrawal CKB token via SUDT fields
            return Err(WithdrawalError::WithdrawFakedCKB.into());
        }

        Ok(())
    }
}
