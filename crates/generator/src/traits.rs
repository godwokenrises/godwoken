use crate::error::{AccountError, DepositError, Error, WithdrawalError};
use crate::sudt::build_l2_sudt_script;
use gw_common::ckb_decimal::{CKBCapacity, CKB_DECIMAL_POW_EXP};
use gw_common::registry::context::RegistryContext;
use gw_common::registry_address::RegistryAddress;
use gw_common::{builtins::CKB_SUDT_ACCOUNT_ID, state::State, CKB_SUDT_SCRIPT_ARGS, H256};
use gw_store::state::traits::JournalDB;
use gw_traits::CodeStore;
use gw_types::U256;
use gw_types::{
    core::ScriptHashType,
    packed::{AccountMerkleState, DepositRequest, Script, WithdrawalReceipt, WithdrawalRequest},
    prelude::*,
};
use gw_utils::RollupContext;
use tracing::instrument;

pub trait StateExt {
    fn create_account_from_script(&mut self, script: Script) -> Result<u32, Error>;
    fn calculate_merkle_state(&self) -> Result<AccountMerkleState, Error>;
    fn apply_deposit_request(
        &mut self,
        ctx: &RollupContext,
        deposit_request: &DepositRequest,
    ) -> Result<(), Error>;

    fn apply_withdrawal_request(
        &mut self,
        ctx: &RollupContext,
        block_producer: &RegistryAddress,
        withdrawal_request: &WithdrawalRequest,
    ) -> Result<WithdrawalReceipt, Error>;

    fn pay_fee(
        &mut self,
        payer: &RegistryAddress,
        block_producer: &RegistryAddress,
        sudt_id: u32,
        amount: U256,
    ) -> Result<(), Error>;
}

impl<S: State + CodeStore + JournalDB> StateExt for S {
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

    /// return current merkle state
    fn calculate_merkle_state(&self) -> Result<AccountMerkleState, Error> {
        let account_root = self.calculate_root()?;
        let account_count = self.get_account_count()?;
        let merkle_state = AccountMerkleState::new_builder()
            .merkle_root(account_root.pack())
            .count(account_count.pack())
            .build();
        Ok(merkle_state)
    }

    fn pay_fee(
        &mut self,
        payer: &RegistryAddress,
        block_producer: &RegistryAddress,
        sudt_id: u32,
        amount: U256,
    ) -> Result<(), Error> {
        log::debug!(
            "account: 0x{} pay fee to block_producer: 0x{}, sudt_id: {}, amount: {}",
            hex::encode(&payer.address),
            hex::encode(&block_producer.address),
            sudt_id,
            &amount
        );
        self.burn_sudt(sudt_id, payer, amount)?;
        self.mint_sudt(sudt_id, block_producer, amount)?;
        Ok(())
    }

    #[instrument(skip_all)]
    fn apply_deposit_request(
        &mut self,
        ctx: &RollupContext,
        request: &DepositRequest,
    ) -> Result<(), Error> {
        // find or create user account
        let account_script_hash: H256 = request.script().hash().into();
        // mint CKB
        let capacity: u64 = request.capacity().unpack();
        log::debug!("[generator] deposit capacity {}", capacity);

        // NOTE: the address length `20` is a hard-coded value, we may re-visit here to extend more address format
        let address = match self.get_account_id_by_script_hash(&account_script_hash)? {
            Some(_id) => {
                // account is exist, query registry address
                self.get_registry_address_by_script_hash(
                    request.registry_id().unpack(),
                    &account_script_hash,
                )?
                .ok_or(Error::Account(AccountError::RegistryAddressNotFound))?
            }
            None => {
                // account isn't exist
                self.insert_script(account_script_hash, request.script());
                let new_id = self.create_account(account_script_hash)?;
                log::debug!(
                    "[generator] create new account: {} id: {}",
                    hex::encode(account_script_hash.as_slice()),
                    new_id
                );
                let registry_ctx = RegistryContext::new(
                    ctx.rollup_config
                        .allowed_eoa_type_hashes()
                        .into_iter()
                        .collect(),
                );
                let addr = registry_ctx.extract_registry_address_from_deposit(
                    request.registry_id().unpack(),
                    &request.script().code_hash(),
                    &request.script().args().raw_data(),
                )?;
                // mapping addr to script hash
                self.mapping_registry_address_to_script_hash(addr.clone(), account_script_hash)?;
                addr
            }
        };
        // Align CKB to 18 decimals
        let ckb_amount = CKBCapacity::from_layer1(capacity).to_layer2();
        self.mint_sudt(CKB_SUDT_ACCOUNT_ID, &address, ckb_amount)?;
        log::debug!(
            "[generator] mint {} shannons * 10^{} CKB to account {}",
            capacity,
            CKB_DECIMAL_POW_EXP,
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
            self.mint_sudt(sudt_id, &address, amount.into())?;
            log::debug!(
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

    #[instrument(skip_all)]
    fn apply_withdrawal_request(
        &mut self,
        ctx: &RollupContext,
        block_producer_address: &RegistryAddress,
        request: &WithdrawalRequest,
    ) -> Result<WithdrawalReceipt, Error> {
        let raw = request.raw();
        let account_script_hash: H256 = raw.account_script_hash().unpack();
        let l2_sudt_script_hash: [u8; 32] =
            build_l2_sudt_script(ctx, &raw.sudt_script_hash().unpack()).hash();
        let amount: u128 = raw.amount().unpack();
        let withdrawal_address = self
            .get_registry_address_by_script_hash(raw.registry_id().unpack(), &account_script_hash)?
            .ok_or(Error::Account(AccountError::RegistryAddressNotFound))?;
        // find user account
        let id = self
            .get_account_id_by_script_hash(&account_script_hash)?
            .ok_or(AccountError::UnknownAccount)?; // find Simple UDT account
        let capacity: u64 = raw.capacity().unpack();
        // pay fee to block producer
        {
            let fee: U256 = raw.fee().unpack().into();
            self.pay_fee(
                &withdrawal_address,
                block_producer_address,
                CKB_SUDT_ACCOUNT_ID,
                fee,
            )?;
        }
        // burn CKB
        self.burn_sudt(
            CKB_SUDT_ACCOUNT_ID,
            &withdrawal_address,
            CKBCapacity::from_layer1(capacity).to_layer2(),
        )?;
        let sudt_id = self
            .get_account_id_by_script_hash(&l2_sudt_script_hash.into())?
            .ok_or(AccountError::UnknownSUDT)?;
        if sudt_id != CKB_SUDT_ACCOUNT_ID {
            // burn sudt
            self.burn_sudt(sudt_id, &withdrawal_address, amount.into())?;
        } else if amount != 0 {
            return Err(WithdrawalError::WithdrawFakedCKB.into());
        }
        // increase nonce
        let nonce = self.get_nonce(id)?;
        let new_nonce = nonce.checked_add(1).ok_or(WithdrawalError::NonceOverflow)?;
        self.set_nonce(id, new_nonce)?;

        let post_state = {
            self.finalise()?;
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
