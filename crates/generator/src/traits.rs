use crate::error::Error;
use crate::syscalls::RunResult;
use gw_common::{builtins::CKB_SUDT_ACCOUNT_ID, error::Error as StateError, state::State, H256};
use gw_types::{
    bytes::Bytes,
    packed::{DepositionRequest, Script, WithdrawalRequest},
    prelude::*,
};

pub trait CodeStore {
    fn insert_script(&mut self, script_hash: H256, script: Script);
    fn get_script(&self, script_hash: &H256) -> Option<Script>;
    fn insert_code(&mut self, script_hash: H256, code: Bytes);
    fn get_code(&self, script_hash: &H256) -> Option<Bytes>;
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
            self.set_account_count(id)?;
        }
        for (script_hash, script) in &run_result.new_scripts {
            self.insert_script(*script_hash, Script::from_slice(&script).expect("script"));
        }
        for (script_hash, data) in &run_result.new_data {
            self.insert_code(*script_hash, Bytes::from(data.clone()));
        }
        Ok(())
    }

    fn apply_deposition_requests(
        &mut self,
        deposition_requests: &[DepositionRequest],
    ) -> Result<(), Error> {
        for request in deposition_requests {
            // find or create user account
            let account_script_hash = request.script().hash();
            let id = match self.get_account_id_by_script_hash(&account_script_hash.into())? {
                Some(id) => id,
                None => {
                    self.insert_script(account_script_hash.into(), request.script().clone());
                    self.create_account(account_script_hash.into())?
                }
            };
            // mint CKB
            let capacity: u64 = request.capacity().unpack();
            self.mint_sudt(CKB_SUDT_ACCOUNT_ID, id, capacity.into())?;
            // find or create Simple UDT account
            let sudt_script_hash = request.sudt_script().hash();
            let sudt_id = match self.get_account_id_by_script_hash(&sudt_script_hash.into())? {
                Some(id) => id,
                None => {
                    self.insert_script(sudt_script_hash.into(), request.sudt_script().clone());
                    self.create_account(sudt_script_hash.into())?
                }
            };
            // mint SUDT
            self.mint_sudt(sudt_id, id, request.amount().unpack())?;
        }

        Ok(())
    }

    fn apply_withdrawal_requests(
        &mut self,
        withdrawal_requests: &[WithdrawalRequest],
    ) -> Result<(), Error> {
        for request in withdrawal_requests {
            let raw = request.raw();
            let account_script_hash: [u8; 32] = raw.account_script_hash().unpack();
            let sudt_script_hash: [u8; 32] = raw.sudt_script_hash().unpack();
            let amount: u128 = raw.amount().unpack();
            // find user account
            let id = self
                .get_account_id_by_script_hash(&account_script_hash.into())?
                .ok_or(StateError::MissingKey)?; // find Simple UDT account
            let capacity: u64 = raw.capacity().unpack();
            // burn CKB
            self.burn_sudt(CKB_SUDT_ACCOUNT_ID, id, capacity.into())?;
            let sudt_id = self
                .get_account_id_by_script_hash(&sudt_script_hash.into())?
                .ok_or(StateError::MissingKey)?;
            // burn sudt
            self.burn_sudt(sudt_id, id, amount)?;
        }

        Ok(())
    }
}
