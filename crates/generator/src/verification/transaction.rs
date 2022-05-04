use gw_common::{
    builtins::{CKB_SUDT_ACCOUNT_ID, ETH_REGISTRY_ACCOUNT_ID},
    state::State,
};
use gw_traits::CodeStore;
use gw_types::{offchain::RollupContext, packed::L2Transaction, prelude::*};
use tracing::instrument;

use crate::{
    constants::MAX_TX_SIZE,
    error::{AccountError, TransactionError, TransactionValidateError},
    typed_transaction::types::TypedRawTransaction,
    utils::get_tx_type,
};

use super::chain_id::ChainIdVerifier;

pub struct TransactionVerifier<'a, S> {
    state: &'a S,
    rollup_context: &'a RollupContext,
}

impl<'a, S: State + CodeStore> TransactionVerifier<'a, S> {
    pub fn new(state: &'a S, rollup_context: &'a RollupContext) -> Self {
        Self {
            state,
            rollup_context,
        }
    }
    /// verify transaction
    /// Notice this function do not perform signature check
    #[instrument(skip_all)]
    pub fn verify(&self, tx: &L2Transaction) -> Result<(), TransactionValidateError> {
        let raw_tx = tx.raw();
        let sender_id: u32 = raw_tx.from_id().unpack();

        // check tx size
        if tx.as_slice().len() > MAX_TX_SIZE {
            return Err(TransactionError::ExceededMaxTxSize {
                max_size: MAX_TX_SIZE,
                tx_size: tx.as_slice().len(),
            }
            .into());
        }

        // check chain_id
        ChainIdVerifier::new(self.rollup_context.rollup_config.chain_id().unpack())
            .verify(raw_tx.chain_id().unpack())?;

        // verify nonce
        let account_nonce: u32 = self.state.get_nonce(sender_id)?;
        let nonce: u32 = raw_tx.nonce().unpack();
        if nonce != account_nonce {
            return Err(TransactionError::Nonce {
                expected: account_nonce,
                actual: nonce,
                account_id: sender_id,
            }
            .into());
        }

        // verify balance
        let sender_script_hash = self.state.get_script_hash(sender_id)?;
        let sender_address = self
            .state
            .get_registry_address_by_script_hash(ETH_REGISTRY_ACCOUNT_ID, &sender_script_hash)?
            .ok_or(AccountError::RegistryAddressNotFound)?;
        let balance = self
            .state
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &sender_address)?;
        // get balance
        let tx_cost = {
            let tx_type = get_tx_type(self.rollup_context, self.state, &tx.raw())?;
            let typed_tx = TypedRawTransaction::from_tx(tx.raw(), tx_type)
                .ok_or(AccountError::UnknownScript)?;
            // reject txs has no cost, these transaction can only be execute without modify state tree
            typed_tx
                .cost()
                .map(Into::into)
                .ok_or(TransactionError::NoCost)?
        };
        if balance < tx_cost {
            return Err(TransactionError::InsufficientBalance.into());
        }

        Ok(())
    }
}
