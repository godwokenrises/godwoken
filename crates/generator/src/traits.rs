use crate::sudt::build_l2_sudt_script;
use crate::{
    error::{AccountError, DepositError, Error, WithdrawalError},
    RollupContext,
};
use gw_common::{builtins::CKB_SUDT_ACCOUNT_ID, state::State, CKB_SUDT_SCRIPT_ARGS};
use gw_traits::CodeStore;
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    offchain::RunResult,
    packed::{AccountMerkleState, DepositRequest, Script, WithdrawalReceipt, WithdrawalRequest},
    prelude::*,
};

pub trait StateExt {
    fn create_account_from_script(&mut self, script: Script) -> Result<u32, Error>;
    fn apply_run_result(&mut self, run_result: &RunResult) -> Result<(), Error>;
    fn apply_deposit_request(
        &mut self,
        ctx: &RollupContext,
        deposit_request: &DepositRequest,
    ) -> Result<(), Error>;

    fn apply_withdrawal_request(
        &mut self,
        ctx: &RollupContext,
        withdrawal_request: &WithdrawalRequest,
    ) -> Result<WithdrawalReceipt, Error>;

    fn apply_deposit_requests(
        &mut self,
        ctx: &RollupContext,
        deposit_requests: &[DepositRequest],
    ) -> Result<(), Error> {
        for request in deposit_requests {
            self.apply_deposit_request(ctx, request)?;
        }
        Ok(())
    }

    fn apply_withdrawal_requests(
        &mut self,
        ctx: &RollupContext,
        withdrawal_requests: &[WithdrawalRequest],
    ) -> Result<Vec<WithdrawalReceipt>, Error> {
        let mut receipts = Vec::with_capacity(withdrawal_requests.len());

        for request in withdrawal_requests {
            let receipt = self.apply_withdrawal_request(ctx, request)?;
            receipts.push(receipt);
        }

        Ok(receipts)
    }
}

impl<S: State + CodeStore> StateExt for S {
    fn create_account_from_script(&mut self, script: Script) -> Result<u32, Error> {
        // Godwoken requires account's script using ScriptHashType::Type
        if script.hash_type() != ScriptHashType::Type.into() {
            return Err(AccountError::UnknownScript.into());
        }
        let script_hash = script.hash();
        self.insert_script(script_hash.into(), script);
        let id = self.create_account(script_hash.into())?;
        Ok(id)
    }

    fn apply_run_result(&mut self, run_result: &RunResult) -> Result<(), Error> {
        for (k, v) in &run_result.write_values {
            self.update_raw(*k, *v)?;
        }
        if let Some(id) = run_result.account_count {
            self.set_account_count(id)?;
        }
        for (script_hash, script) in &run_result.new_scripts {
            self.insert_script(*script_hash, Script::from_slice(&script).expect("script"));
        }
        for (data_hash, data) in &run_result.write_data {
            // register data hash into SMT
            self.store_data_hash(*data_hash)?;
            self.insert_data(*data_hash, Bytes::from(data.clone()));
        }
        Ok(())
    }

    fn apply_deposit_request(
        &mut self,
        ctx: &RollupContext,
        request: &DepositRequest,
    ) -> Result<(), Error> {
        // find or create user account
        let account_script_hash = request.script().hash();
        // mint CKB
        let capacity: u64 = request.capacity().unpack();
        if self.get_account_id_by_script_hash(&account_script_hash.into())?.is_none() {
            self.insert_script(account_script_hash.into(), request.script());
            let _new_id = self.create_account(account_script_hash.into())?;
        }
        // NOTE: the length `20` is a hard-coded value, may be `16` for some LockAlgorithm.
        let short_address = &account_script_hash[0..20];
        self.mint_sudt(CKB_SUDT_ACCOUNT_ID, short_address, capacity.into())?;
        let sudt_script_hash = request.sudt_script_hash().unpack();
        let amount = request.amount().unpack();
        if sudt_script_hash != CKB_SUDT_SCRIPT_ARGS.into() {
            // find or create Simple UDT account
            let l2_sudt_script = build_l2_sudt_script(&ctx, &sudt_script_hash);
            let l2_sudt_script_hash: [u8; 32] = l2_sudt_script.hash();
            let sudt_id = match self.get_account_id_by_script_hash(&l2_sudt_script_hash.into())? {
                Some(id) => id,
                None => {
                    self.insert_script(l2_sudt_script_hash.into(), l2_sudt_script);
                    self.create_account(l2_sudt_script_hash.into())?
                }
            };
            // prevent fake CKB SUDT, the caller should filter these invalid deposits
            if sudt_id == CKB_SUDT_ACCOUNT_ID {
                return Err(AccountError::InvalidSUDTOperation.into());
            }
            // mint SUDT
            self.mint_sudt(sudt_id, short_address, amount)?;
        } else if amount != 0 {
            return Err(DepositError::DepositFakedCKB.into());
        }

        Ok(())
    }

    fn apply_withdrawal_request(
        &mut self,
        ctx: &RollupContext,
        request: &WithdrawalRequest,
    ) -> Result<WithdrawalReceipt, Error> {
        let raw = request.raw();
        let account_script_hash: [u8; 32] = raw.account_script_hash().unpack();
        let l2_sudt_script_hash: [u8; 32] =
            build_l2_sudt_script(&ctx, &raw.sudt_script_hash().unpack()).hash();
        let amount: u128 = raw.amount().unpack();
        // find user account
        let id = self
            .get_account_id_by_script_hash(&account_script_hash.into())?
            .ok_or(AccountError::UnknownAccount)?; // find Simple UDT account
        let capacity: u64 = raw.capacity().unpack();
        // NOTE: the length `20` is a hard-coded value, may be `16` for some LockAlgorithm.
        let short_address = &account_script_hash[0..20];
        // burn CKB
        self.burn_sudt(CKB_SUDT_ACCOUNT_ID, short_address, capacity.into())?;
        let sudt_id = self
            .get_account_id_by_script_hash(&l2_sudt_script_hash.into())?
            .ok_or(AccountError::UnknownSUDT)?;
        if sudt_id != CKB_SUDT_ACCOUNT_ID {
            // burn sudt
            self.burn_sudt(sudt_id, short_address, amount)?;
        } else if amount != 0 {
            return Err(WithdrawalError::WithdrawFakedCKB.into());
        }
        // increase nonce
        let nonce = self.get_nonce(id)?;
        let new_nonce = nonce.checked_add(1).ok_or(AccountError::NonceOverflow)?;
        self.set_nonce(id, new_nonce)?;

        let post_state = {
            let account_root = self.calculate_root()?;
            let account_count = self.get_account_count()?;
            AccountMerkleState::new_builder()
                .merkle_root(account_root.pack())
                .count(account_count.pack())
                .build()
        };

        let receipt = WithdrawalReceipt::new_builder()
            .post_state(post_state)
            .build();

        Ok(receipt)
    }
}
