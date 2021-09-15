use crate::error::{AccountError, DepositError, Error, WithdrawalError};
use crate::sudt::build_l2_sudt_script;
use gw_common::{
    builtins::CKB_SUDT_ACCOUNT_ID,
    state::{to_short_address, State},
    CKB_SUDT_SCRIPT_ARGS, H256,
};
use gw_traits::CodeStore;
use gw_types::offchain::RollupContext;
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    offchain::RunResult,
    packed::{AccountMerkleState, DepositRequest, Script, WithdrawalReceipt, WithdrawalRequest},
    prelude::*,
};

pub trait StateExt {
    fn create_account_from_script(&mut self, script: Script) -> Result<u32, Error>;
    fn merkle_state(&self) -> Result<AccountMerkleState, Error>;
    fn apply_run_result(&mut self, run_result: &RunResult) -> Result<(), Error>;
    fn apply_deposit_request(
        &mut self,
        ctx: &RollupContext,
        deposit_request: &DepositRequest,
    ) -> Result<(), Error>;

    fn apply_withdrawal_request(
        &mut self,
        ctx: &RollupContext,
        block_producer_id: u32,
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

    fn pay_fee(
        &mut self,
        payer_short_address: &[u8],
        block_producer_short_address: &[u8],
        sudt_id: u32,
        amount: u128,
    ) -> Result<(), Error>;

    fn apply_withdrawal_requests(
        &mut self,
        ctx: &RollupContext,
        block_producer_id: u32,
        withdrawal_requests: &[WithdrawalRequest],
    ) -> Result<Vec<WithdrawalReceipt>, Error> {
        let mut receipts = Vec::with_capacity(withdrawal_requests.len());

        for request in withdrawal_requests {
            let receipt = self.apply_withdrawal_request(ctx, block_producer_id, request)?;
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

    fn merkle_state(&self) -> Result<AccountMerkleState, Error> {
        let account_root = self.calculate_root()?;
        let account_count = self.get_account_count()?;
        let merkle_state = AccountMerkleState::new_builder()
            .merkle_root(account_root.pack())
            .count(account_count.pack())
            .build();
        Ok(merkle_state)
    }

    fn apply_run_result(&mut self, run_result: &RunResult) -> Result<(), Error> {
        for (k, v) in &run_result.write_values {
            self.update_raw(*k, *v)?;
        }
        if let Some(id) = run_result.account_count {
            self.set_account_count(id)?;
        }
        for (script_hash, script) in &run_result.new_scripts {
            self.insert_script(*script_hash, Script::from_slice(script).expect("script"));
        }
        for (data_hash, data) in &run_result.write_data {
            self.insert_data(*data_hash, Bytes::from(data.clone()));
        }

        Ok(())
    }

    fn pay_fee(
        &mut self,
        payer_short_address: &[u8],
        block_producer_short_address: &[u8],
        sudt_id: u32,
        amount: u128,
    ) -> Result<(), Error> {
        log::debug!(
            "account: 0x{} pay fee to block_producer: 0x{}, sudt_id: {}, amount: {}",
            hex::encode(&payer_short_address),
            hex::encode(&block_producer_short_address),
            sudt_id,
            &amount
        );
        self.burn_sudt(sudt_id, payer_short_address, amount)?;
        self.mint_sudt(sudt_id, block_producer_short_address, amount)?;
        Ok(())
    }

    fn apply_deposit_request(
        &mut self,
        ctx: &RollupContext,
        request: &DepositRequest,
    ) -> Result<(), Error> {
        // find or create user account
        let account_script_hash: H256 = request.script().hash().into();
        // mint CKB
        let capacity: u64 = request.capacity().unpack();
        if self
            .get_account_id_by_script_hash(&account_script_hash)?
            .is_none()
        {
            self.insert_script(account_script_hash, request.script());
            let new_id = self.create_account(account_script_hash)?;
            log::info!(
                "[generator] create new account: {} id: {}",
                hex::encode(account_script_hash.as_slice()),
                new_id
            );
        }
        // NOTE: the length `20` is a hard-coded value, may be `16` for some LockAlgorithm.
        self.mint_sudt(
            CKB_SUDT_ACCOUNT_ID,
            to_short_address(&account_script_hash),
            capacity.into(),
        )?;
        log::info!(
            "[generator] mint {} shannons CKB to account {}",
            capacity,
            hex::encode(account_script_hash.as_slice()),
        );
        let sudt_script_hash = request.sudt_script_hash().unpack();
        let amount = request.amount().unpack();
        if sudt_script_hash != CKB_SUDT_SCRIPT_ARGS.into() {
            // find or create Simple UDT account
            let l2_sudt_script = build_l2_sudt_script(ctx, &sudt_script_hash);
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
            self.mint_sudt(sudt_id, to_short_address(&account_script_hash), amount)?;
            log::info!(
                "[generator] mint {} amount sUDT {} to account {}",
                amount,
                sudt_id,
                hex::encode(account_script_hash.as_slice()),
            );
        } else if amount != 0 {
            return Err(DepositError::DepositFakedCKB.into());
        }

        Ok(())
    }

    fn apply_withdrawal_request(
        &mut self,
        ctx: &RollupContext,
        block_producer_id: u32,
        request: &WithdrawalRequest,
    ) -> Result<WithdrawalReceipt, Error> {
        let raw = request.raw();
        let account_script_hash: H256 = raw.account_script_hash().unpack();
        let l2_sudt_script_hash: [u8; 32] =
            build_l2_sudt_script(ctx, &raw.sudt_script_hash().unpack()).hash();
        let amount: u128 = raw.amount().unpack();
        let withdrawal_short_address = to_short_address(&account_script_hash);
        // find user account
        let id = self
            .get_account_id_by_script_hash(&account_script_hash)?
            .ok_or(AccountError::UnknownAccount)?; // find Simple UDT account
        let capacity: u64 = raw.capacity().unpack();
        // pay fee to block producer
        {
            let sudt_id: u32 = raw.fee().sudt_id().unpack();
            let amount: u128 = raw.fee().amount().unpack();
            let block_producer_script_hash = self.get_script_hash(block_producer_id)?;
            let block_producer_short_address = to_short_address(&block_producer_script_hash);
            self.pay_fee(
                withdrawal_short_address,
                block_producer_short_address,
                sudt_id,
                amount,
            )?;
        }
        // burn CKB
        self.burn_sudt(
            CKB_SUDT_ACCOUNT_ID,
            withdrawal_short_address,
            capacity.into(),
        )?;
        let sudt_id = self
            .get_account_id_by_script_hash(&l2_sudt_script_hash.into())?
            .ok_or(AccountError::UnknownSUDT)?;
        if sudt_id != CKB_SUDT_ACCOUNT_ID {
            // burn sudt
            self.burn_sudt(sudt_id, withdrawal_short_address, amount)?;
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
