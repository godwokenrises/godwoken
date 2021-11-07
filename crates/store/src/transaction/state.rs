use gw_common::{smt::SMT, H256};
use gw_db::{
    error::Error,
    schema::{
        COLUMN_ACCOUNT_SMT_BRANCH, COLUMN_ACCOUNT_SMT_LEAF, COLUMN_BLOCK_STATE_RECORD,
        COLUMN_BLOCK_STATE_REVERSE_RECORD,
    },
    DBRawIterator, Direction, IteratorMode, ReadOptions,
};
use gw_types::{packed::AccountMerkleState, prelude::*};

use super::StoreTransaction;
use crate::{
    smt::smt_store::SMTStore,
    state::state_db::{StateContext, StateTree},
    traits::KVStore,
};

/// TODO use a variable instead of hardcode
const NUMBER_OF_CONFIRMATION: u64 = 3600;

impl StoreTransaction {
    pub fn account_smt_store(&self) -> Result<SMTStore<'_, Self>, Error> {
        let smt_store = SMTStore::new(COLUMN_ACCOUNT_SMT_LEAF, COLUMN_ACCOUNT_SMT_BRANCH, self);
        Ok(smt_store)
    }

    pub fn account_smt_with_merkle_state(
        &self,
        merkle_state: AccountMerkleState,
    ) -> Result<SMT<SMTStore<'_, Self>>, Error> {
        let smt_store = self.account_smt_store()?;
        Ok(SMT::new(merkle_state.merkle_root().unpack(), smt_store))
    }

    // get state tree
    pub fn state_tree(&self, context: StateContext) -> Result<StateTree<'_>, Error> {
        let block = match context {
            StateContext::ReadOnlyHistory(block_number) => {
                let block_hash = self
                    .get_block_hash_by_number(block_number)?
                    .ok_or_else(|| Error::from("can't find block".to_string()))?;
                self.get_block(&block_hash)?
                    .ok_or_else(|| "can't find block".to_string())?
            }
            _ => self.get_tip_block()?,
        };
        let merkle_state = block.raw().post_account();
        let account_count = merkle_state.count().unpack();
        let tree = self.account_smt_with_merkle_state(merkle_state)?;
        Ok(StateTree::new(tree, account_count, context))
    }

    // FIXME: This method may running into inconsistent state if current state is dirty.
    // We should separate the StateDB into ReadOnly & WriteOnly,
    // The ReadOnly is for fetching history state, and the write only is for writing new state.
    // This function should only be added on the ReadOnly state.
    pub fn account_smt(&self) -> Result<SMT<SMTStore<'_, Self>>, Error> {
        let block = self.get_tip_block()?;
        let merkle_state = block.raw().post_account();
        self.account_smt_with_merkle_state(merkle_state)
    }

    pub(crate) fn record_block_state(
        &self,
        block_number: u64,
        state_key: H256,
        value: H256,
    ) -> Result<(), Error> {
        // insert block state key value
        let key = BlockStateRecordKey::new(block_number, &state_key);
        self.insert_raw(COLUMN_BLOCK_STATE_RECORD, key.as_slice(), value.as_slice())?;
        // insert block state reverse key
        let reverse_key = BlockStateRecordKeyReverse::new(block_number, &state_key);
        self.insert_raw(
            COLUMN_BLOCK_STATE_REVERSE_RECORD,
            reverse_key.as_slice(),
            &[],
        )
    }

    /// Prune finalized block state record
    /// The arg new_number is current block number
    pub(crate) fn prune_finalized_block_state_record(&self, new_number: u64) -> Result<(), Error> {
        if new_number <= NUMBER_OF_CONFIRMATION {
            return Ok(());
        }
        let finalized_block_number = new_number - NUMBER_OF_CONFIRMATION - 1;
        if finalized_block_number == 0 {
            return Ok(());
        }
        self.remove_block_state_record(finalized_block_number)
    }

    //     pub fn state_tree(&self) -> Result<StateTree<'_>, Error> {
    //         let merkle_state = self.get_checkpoint_merkle_state()?;
    //         self.state_tree_with_merkle_state(merkle_state)
    //     }
    pub(crate) fn remove_block_state_record(&self, block_number: u64) -> Result<(), Error> {
        let iter = self.iter_block_state_record(block_number);
        for record_key in iter {
            // delete record key
            self.delete(COLUMN_BLOCK_STATE_RECORD, record_key.as_slice())?;
            // delete reverse record key
            let reverse_key =
                BlockStateRecordKeyReverse::new(record_key.block_number(), &record_key.state_key());
            self.delete(COLUMN_BLOCK_STATE_REVERSE_RECORD, reverse_key.as_slice())?;
        }
        Ok(())
    }

    pub fn get_history_state(&self, block_number: u64, state_key: &H256) -> Option<H256> {
        let key = BlockStateRecordKeyReverse::new(block_number, state_key);
        let mut opts = ReadOptions::default();
        opts.set_total_order_seek(false);
        let mut raw_iter: DBRawIterator = self
            .get_iter_opts(
                COLUMN_BLOCK_STATE_REVERSE_RECORD,
                IteratorMode::Start,
                &opts,
            )
            .into();
        raw_iter.seek_for_prev(key.as_slice());

        if !raw_iter.valid() {
            return None;
        }
        match raw_iter.key() {
            Some(prev_key) => {
                // not a some key
                if &prev_key[..32] != key.state_key().as_slice() {
                    return None;
                }

                // get old value
                let prev_reverse_key = BlockStateRecordKeyReverse::from_slice(prev_key);
                let prev_key = BlockStateRecordKey::new(
                    prev_reverse_key.block_number(),
                    &prev_reverse_key.state_key(),
                );

                self.get(COLUMN_BLOCK_STATE_RECORD, prev_key.as_slice())
                    .map(|raw| {
                        let mut buf = [0u8; 32];
                        buf.copy_from_slice(&raw);
                        buf.into()
                    })
            }
            _ => None,
        }
    }

    pub(crate) fn iter_block_state_record(
        &self,
        block_number: u64,
    ) -> impl Iterator<Item = BlockStateRecordKey> + '_ {
        let start_key = BlockStateRecordKey::new(block_number, &H256::zero());
        let mut opts = ReadOptions::default();
        opts.set_total_order_seek(false);
        self.get_iter_opts(
            COLUMN_BLOCK_STATE_RECORD,
            IteratorMode::From(start_key.as_slice(), Direction::Forward),
            &opts,
        )
        .map(|(key, _value)| BlockStateRecordKey::from_slice(&key))
        .take_while(move |key| key.block_number() == block_number)
    }
}

// block_number(8 bytes) | key (32 bytes)
pub(crate) struct BlockStateRecordKey([u8; 40]);

impl BlockStateRecordKey {
    pub fn new(block_number: u64, state_key: &H256) -> Self {
        let mut inner = [0u8; 40];
        inner[..8].copy_from_slice(&block_number.to_be_bytes());
        inner[8..].copy_from_slice(state_key.as_slice());
        BlockStateRecordKey(inner)
    }

    pub fn state_key(&self) -> H256 {
        let mut inner = [0u8; 32];
        inner.copy_from_slice(&self.0[8..]);
        inner.into()
    }

    pub fn block_number(&self) -> u64 {
        let mut inner = [0u8; 8];
        inner.copy_from_slice(&self.0[..8]);
        u64::from_be_bytes(inner)
    }

    pub fn from_slice(bytes: &[u8]) -> Self {
        let mut inner = [0u8; 40];
        inner.copy_from_slice(bytes);
        BlockStateRecordKey(inner)
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

//  key (32 bytes) | block_number(8 bytes)
pub(crate) struct BlockStateRecordKeyReverse([u8; 40]);

impl BlockStateRecordKeyReverse {
    pub fn new(block_number: u64, state_key: &H256) -> Self {
        let mut inner = [0u8; 40];
        inner[..32].copy_from_slice(state_key.as_slice());
        inner[32..].copy_from_slice(&block_number.to_be_bytes());
        BlockStateRecordKeyReverse(inner)
    }

    fn state_key(&self) -> H256 {
        let mut inner = [0u8; 32];
        inner.copy_from_slice(&self.0[..32]);
        inner.into()
    }

    fn block_number(&self) -> u64 {
        let mut inner = [0u8; 8];
        inner.copy_from_slice(&self.0[32..]);
        u64::from_be_bytes(inner)
    }

    fn from_slice(bytes: &[u8]) -> Self {
        let mut inner = [0u8; 40];
        inner.copy_from_slice(bytes);
        BlockStateRecordKeyReverse(inner)
    }

    fn as_slice(&self) -> &[u8] {
        &self.0
    }
}
