//! State DB

use crate::transaction::state::BlockStateRecordKeyReverse;
use crate::{smt::smt_store::SMTStore, traits::KVStore, transaction::StoreTransaction};
use anyhow::{anyhow, Result};
use gw_common::merkle_utils::calculate_state_checkpoint;
use gw_common::{error::Error as StateError, smt::SMT, state::State, H256};
use gw_db::schema::{
    Col, COLUMN_ACCOUNT_SMT_BRANCH, COLUMN_ACCOUNT_SMT_LEAF, COLUMN_DATA, COLUMN_SCRIPT,
    COLUMN_SCRIPT_PREFIX,
};
use gw_db::{error::Error, iter::DBIter, DBRawIterator, IteratorMode};
use gw_traits::CodeStore;
use gw_types::{
    bytes::Bytes,
    packed::{self, AccountMerkleState, L2Block},
    prelude::*,
};
use std::{cell::RefCell, collections::HashSet, fmt, mem::size_of_val};

use super::state_tracker::StateTracker;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum StateContext {
    ReadOnly,
    ReadOnlyHistory(u64),
    AttachBlock(u64),
    DetachBlock(u64),
}

pub struct StateTree<'a> {
    tree: SMT<SMTStore<'a, StoreTransaction>>,
    account_count: u32,
    context: StateContext,
}

impl<'a> StateTree<'a> {
    pub fn new(
        tree: SMT<SMTStore<'a, StoreTransaction>>,
        account_count: u32,
        context: StateContext,
    ) -> Self {
        StateTree {
            tree,
            account_count,
            context,
        }
    }

    pub fn get_merkle_state(&self) -> AccountMerkleState {
        AccountMerkleState::new_builder()
            .merkle_root(self.tree.root().pack())
            .count(self.account_count.pack())
            .build()
    }

    /// Detach block state from state tree
    pub fn detach_block_state(&mut self) -> Result<()> {
        let block_number = match self.context {
            StateContext::DetachBlock(block_number) => block_number,
            ctx => return Err(anyhow!("Wrong context in detach block state: {:?}", ctx)),
        };

        // reset states to previous value
        let parent_block_number = block_number.saturating_sub(1);
        let reverted_key_values: Vec<_> = self
            .db()
            .iter_block_state_record(block_number)
            .map(|record_key| {
                let state_key = record_key.state_key();
                let last_value = self
                    .db()
                    .get_history_state(parent_block_number, &state_key)
                    .unwrap_or(H256::zero());
                (state_key, last_value)
            })
            .collect();
        for (state_key, last_value) in reverted_key_values {
            self.update_raw(state_key, last_value)?;
        }

        // remove block's state record
        self.db().remove_block_state_record(block_number)?;

        Ok(())
    }

    fn db(&self) -> &StoreTransaction {
        &self.tree.store().inner_store()
    }
}

impl<'a> State for StateTree<'a> {
    fn get_raw(&self, key: &H256) -> Result<H256, StateError> {
        let v = match self.context {
            StateContext::ReadOnlyHistory(block_number) => self
                .db()
                .get_history_state(block_number, key)
                .unwrap_or_default(),
            _ => self.tree.get(key)?,
        };
        Ok(v)
    }

    fn update_raw(&mut self, key: H256, value: H256) -> Result<(), StateError> {
        self.tree.update(key, value)?;
        // record block's kv state
        match self.context {
            StateContext::AttachBlock(block_number) => {
                self.db()
                    .record_block_state(block_number, key, value)
                    .expect("record block state");
            }
            StateContext::DetachBlock(_) => {
                // ignore
            }
            ctx => {
                panic!("wrong ctx: {:?}", ctx);
            }
        }
        Ok(())
    }

    fn get_account_count(&self) -> Result<u32, StateError> {
        Ok(self.account_count)
    }

    fn set_account_count(&mut self, count: u32) -> Result<(), StateError> {
        self.account_count = count;
        Ok(())
    }

    fn calculate_root(&self) -> Result<H256, StateError> {
        let root = self.tree.root();
        Ok(*root)
    }
}

impl<'a> CodeStore for StateTree<'a> {
    fn insert_script(&mut self, script_hash: H256, script: packed::Script) {
        self.db()
            .insert_raw(COLUMN_SCRIPT, script_hash.as_slice(), script.as_slice())
            .expect("insert script");

        // build script_hash prefix search index
        self.db()
            .insert_raw(
                COLUMN_SCRIPT_PREFIX,
                &script_hash.as_slice()[..20],
                script_hash.as_slice(),
            )
            .expect("insert script prefix");
    }

    fn get_script(&self, script_hash: &H256) -> Option<packed::Script> {
        self.db()
            .get(COLUMN_SCRIPT, script_hash.as_slice())
            .map(|slice| packed::ScriptReader::from_slice_should_be_ok(slice.as_ref()).to_entity())
    }

    fn get_script_hash_by_short_address(&self, script_hash_prefix: &[u8]) -> Option<H256> {
        match self.db().get(COLUMN_SCRIPT_PREFIX, script_hash_prefix) {
            Some(slice) => {
                let mut hash = [0u8; 32];
                hash.copy_from_slice(slice.as_ref());
                Some(hash.into())
            }
            None => None,
        }
    }

    fn insert_data(&mut self, data_hash: H256, code: Bytes) {
        self.db()
            .insert_raw(COLUMN_DATA, data_hash.as_slice(), &code)
            .expect("insert data");
    }

    fn get_data(&self, data_hash: &H256) -> Option<Bytes> {
        self.db()
            .get(COLUMN_DATA, data_hash.as_slice())
            .map(|slice| Bytes::from(slice.to_vec()))
    }
}
