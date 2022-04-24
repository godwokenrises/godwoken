use gw_common::state::State;
use gw_traits::CodeStore;
use gw_types::{offchain::RollupContext, packed::L2Transaction, prelude::*};
use tracing::instrument;

use crate::{
    constants::MAX_TX_SIZE,
    error::{TransactionError, TransactionValidateError},
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
        // TODO
        // get balance

        Ok(())
    }
}
