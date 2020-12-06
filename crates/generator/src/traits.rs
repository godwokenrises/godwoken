use crate::error::{Error, ValidateError};
use crate::generator::DepositionRequest;
use crate::syscalls::RunResult;
use crate::{bytes::Bytes, generator::WithdrawalRequest};
use gw_common::{error::Error as StateError, state::State, FINALITY_BLOCKS, H256};
use gw_types::{packed::Script, prelude::*};

pub trait CodeStore {
    fn insert_script(&mut self, script_hash: H256, script: Script);
    fn insert_code(&mut self, code_hash: H256, code: Bytes);
    fn get_script(&self, script_hash: &H256) -> Option<Script>;
    fn get_code(&self, code_hash: &H256) -> Option<Bytes>;
    fn get_code_by_script_hash(&self, script_hash: &H256) -> Option<Bytes> {
        self.get_script(script_hash)
            .and_then(|script| self.get_code(&script.code_hash().unpack().into()))
    }
}

pub trait StateExt {
    fn create_account_from_script(&mut self, script: Script) -> Result<u32, Error>;
    fn apply_run_result(&mut self, run_result: &RunResult) -> Result<(), Error>;
    fn apply_deposition_requests(
        &mut self,
        deposition_requests: &[DepositionRequest],
    ) -> Result<(), Error>;

    fn apply_withdrawal_requests(
        &mut self,
        withdrawal_requests: &[WithdrawalRequest],
        block_number: u64,
    ) -> Result<(), Error>;
}

impl<S: State + CodeStore> StateExt for S {
    fn create_account_from_script(&mut self, script: Script) -> Result<u32, Error> {
        let script_hash = script.hash();
        self.insert_script(script_hash.into(), script);
        let id = self.create_account(script_hash.into())?;
        Ok(id)
    }
    fn apply_run_result(&mut self, run_result: &RunResult) -> Result<(), Error> {
        for (k, v) in &run_result.write_values {
            self.update_raw((*k).into(), (*v).into())?;
        }
        if let Some(id) = run_result.account_count {
            self.set_account_count(id);
        }
        for (script_hash, script) in &run_result.new_scripts {
            self.insert_script(*script_hash, Script::from_slice(&script).expect("script"));
        }
        Ok(())
    }

    fn apply_deposition_requests(
        &mut self,
        deposition_requests: &[DepositionRequest],
    ) -> Result<(), Error> {
        for request in deposition_requests {
            // find or create user account
            let account_script_hash = request.script.hash();
            let id = match self.get_account_id_by_script_hash(&account_script_hash.into())? {
                Some(id) => id,
                None => {
                    self.insert_script(account_script_hash.into(), request.script.clone());
                    self.create_account(account_script_hash.into())?
                }
            };
            // find or create Simple UDT account
            let sudt_script_hash = request.sudt_script.hash();
            let sudt_id = match self.get_account_id_by_script_hash(&sudt_script_hash.into())? {
                Some(id) => id,
                None => {
                    self.insert_script(sudt_script_hash.into(), request.sudt_script.clone());
                    self.create_account(sudt_script_hash.into())?
                }
            };
            self.mint_sudt(sudt_id, id, request.amount)?;
        }

        Ok(())
    }

    fn apply_withdrawal_requests(
        &mut self,
        withdrawal_requests: &[WithdrawalRequest],
        block_number: u64,
    ) -> Result<(), Error> {
        let largest_prepare_number = block_number
            .checked_sub(FINALITY_BLOCKS)
            .ok_or(ValidateError::InvalidWithdrawal)?;
        for request in withdrawal_requests {
            // find user account
            let id = self
                .get_account_id_by_script_hash(&request.account_script_hash)?
                .ok_or(StateError::MissingKey)?; // find Simple UDT account
            let sudt_id = self
                .get_account_id_by_script_hash(&request.sudt_script_hash)?
                .ok_or(StateError::MissingKey)?;
            let record = self.get_prepare_withdrawal(sudt_id, id)?;
            // check validity of withdrawal
            if record.amount != request.amount
                || record.withdrawal_lock_hash != request.lock_hash
                || record.block_number > largest_prepare_number
            {
                return Err(ValidateError::InvalidWithdrawal.into());
            }
            // remove prepare withdrawal record
            self.remove_prepare_withdrawal(sudt_id, id)?;
        }

        Ok(())
    }
}
