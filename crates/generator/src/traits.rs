use crate::generator::DepositionRequest;
use crate::syscalls::RunResult;
use crate::{bytes::Bytes, generator::WithdrawalRequest};
use gw_common::{
    state::{Error, State},
    H256,
};
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
    ) -> Result<(), Error>;
}

impl<S: State + CodeStore> StateExt for S {
    fn create_account_from_script(&mut self, script: Script) -> Result<u32, Error> {
        let script_hash = script.hash();
        self.insert_script(script_hash.into(), script);
        self.create_account(script_hash.into())
    }
    fn apply_run_result(&mut self, run_result: &RunResult) -> Result<(), Error> {
        for (k, v) in &run_result.write_values {
            self.update_raw((*k).into(), (*v).into())?;
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
    ) -> Result<(), Error> {
        for request in withdrawal_requests {
            // find user account
            let id = self
                .get_account_id_by_script_hash(&request.account_script_hash)?
                .ok_or(Error::MissingKey)?; // find Simple UDT account
            let sudt_id = self
                .get_account_id_by_script_hash(&request.sudt_script_hash)?
                .ok_or(Error::MissingKey)?;
            self.burn_sudt(sudt_id, id, request.amount)?;
        }

        Ok(())
    }
}
