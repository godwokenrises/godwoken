//! State DB

use crate::constant::MEMORY_BLOCK_NUMBER;
use crate::transaction::state::BlockStateRecordKeyReverse;
use crate::{smt_store_impl::SMTStore, traits::KVStore, transaction::StoreTransaction};
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

const FLAG_DELETE_VALUE: u8 = 0;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct WriteContext {
    pub tx_offset: u32,
}

impl WriteContext {
    pub fn new(tx_offset: u32) -> Self {
        Self { tx_offset }
    }
}

impl Default for WriteContext {
    fn default() -> Self {
        Self { tx_offset: 0 }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum StateDBMode {
    Genesis,
    ReadOnly,
    Write(WriteContext),
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum SubState {
    Withdrawal(u32),
    PrevTxs,
    Tx(u32),
    Block, // Block post state
    MemBlock,
}

impl SubState {
    fn validate_in_block(&self, block: &L2Block) -> Result<()> {
        match self {
            SubState::Withdrawal(index) => {
                if *index as usize >= block.withdrawals().len() {
                    return Err(anyhow!("invalid withdrawal substate index"));
                }
            }
            SubState::Tx(index) => {
                if *index as usize >= block.transactions().len() {
                    return Err(anyhow!("invalid tx substate index"));
                }
            }
            _ => (),
        }

        Ok(())
    }

    fn extract_checkpoint_post_state_from_block(
        &self,
        db: &StoreTransaction,
        block: &L2Block,
    ) -> Result<AccountMerkleState, Error> {
        self.validate_in_block(block).map_err(|e| e.to_string())?;

        enum CheckPointIdx {
            PrevTxs,
            Idx(usize),
        }

        let checkpoint_idx = match *self {
            SubState::Withdrawal(index) => CheckPointIdx::Idx(index as usize),
            SubState::PrevTxs => CheckPointIdx::PrevTxs,
            SubState::Tx(index) => CheckPointIdx::Idx(block.withdrawals().len() + index as usize),
            SubState::Block => return Ok(block.raw().post_account()),
            SubState::MemBlock => {
                let root = db.get_mem_block_account_smt_root()?;
                let count = db.get_mem_block_account_count()?;
                let merkle_state = AccountMerkleState::new_builder()
                    .merkle_root(root.pack())
                    .count(count.pack())
                    .build();
                return Ok(merkle_state);
            }
        };

        let checkpoint = match checkpoint_idx {
            CheckPointIdx::Idx(idx) => {
                let checkpoints = block.as_reader().raw().state_checkpoint_list().to_entity();
                checkpoints.get(idx).expect("checkpoint exists")
            }
            CheckPointIdx::PrevTxs => {
                let txs = block.as_reader().raw().submit_transactions();
                txs.prev_state_checkpoint().to_entity()
            }
        };

        db.get_checkpoint_post_state(&checkpoint)?
            .ok_or_else(|| "can't find checkpoint".to_string().into())
    }
}

/// Tracker state changes
pub struct StateTracker {
    touched_keys: Option<RefCell<HashSet<H256>>>,
}

impl Default for StateTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl StateTracker {
    pub fn new() -> Self {
        StateTracker { touched_keys: None }
    }

    /// Enable state tracking
    pub fn enable(&mut self) {
        if self.touched_keys.is_none() {
            self.touched_keys = Some(Default::default())
        }
    }

    /// Return touched keys
    pub fn touched_keys(&self) -> Option<&RefCell<HashSet<H256>>> {
        self.touched_keys.as_ref()
    }

    /// Record a key in the tracker
    pub fn touch_key(&self, key: &H256) {
        if let Some(touched_keys) = self.touched_keys.as_ref() {
            touched_keys.borrow_mut().insert(*key);
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum StateContext {
    None,
    AttachBlock(u64),
    DetachBlock(u64),
}

pub struct StateTree<'a> {
    tree: SMT<SMTStore<'a, StoreTransaction>>,
    account_count: u32,
    db: &'a StoreTransaction,
    context: StateContext,
    tracker: StateTracker,
}

impl<'a> StateTree<'a> {
    pub fn new(
        db: &'a StoreTransaction,
        tree: SMT<SMTStore<'a, StoreTransaction>>,
        account_count: u32,
        context: StateContext,
    ) -> Self {
        StateTree {
            tree,
            db,
            account_count,
            context,
            tracker: StateTracker::new(),
        }
    }

    pub fn tracker_mut(&mut self) -> &mut StateTracker {
        &mut self.tracker
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
        for record_key in self.db.iter_block_state_record(block_number) {
            let reverse_key =
                BlockStateRecordKeyReverse::new(record_key.block_number(), &record_key.state_key());
            let last_value = self.db.find_state_key_last_value(&reverse_key);
            self.update_raw(record_key.state_key(), last_value.unwrap_or(H256::zero()))?;
        }

        // remove block's state record
        self.db.remove_block_state_record(block_number)?;

        Ok(())
    }

    /// submit tree changes into memory block
    /// notice, this function do not commit the DBTransaction
    pub fn submit_tree_to_mem_block(&self) -> Result<(), Error> {
        self.db
            .set_mem_block_account_smt_root(*self.tree.root())
            .expect("set smt root");
        self.db
            .set_mem_block_account_count(self.account_count)
            .expect("set smt root");
        Ok(())
    }
}

impl<'a> State for StateTree<'a> {
    fn get_raw(&self, key: &H256) -> Result<H256, StateError> {
        self.tracker.touch_key(key);
        let v = self.tree.get(key)?;
        Ok(v)
    }

    fn update_raw(&mut self, key: H256, value: H256) -> Result<(), StateError> {
        self.tracker.touch_key(&key);
        self.tree.update(key, value)?;
        // record block's kv state
        match self.context {
            StateContext::AttachBlock(block_number) => {
                self.db
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
        self.db
            .insert_raw(COLUMN_SCRIPT, script_hash.as_slice(), script.as_slice())
            .expect("insert script");

        // build script_hash prefix search index
        self.db
            .insert_raw(
                COLUMN_SCRIPT_PREFIX,
                &script_hash.as_slice()[..20],
                script_hash.as_slice(),
            )
            .expect("insert script prefix");
    }

    fn get_script(&self, script_hash: &H256) -> Option<packed::Script> {
        self.db
            .get(COLUMN_SCRIPT, script_hash.as_slice())
            .map(|slice| packed::ScriptReader::from_slice_should_be_ok(slice.as_ref()).to_entity())
    }

    fn get_script_hash_by_short_address(&self, script_hash_prefix: &[u8]) -> Option<H256> {
        match self.db.get(COLUMN_SCRIPT_PREFIX, script_hash_prefix) {
            Some(slice) => {
                let mut hash = [0u8; 32];
                hash.copy_from_slice(slice.as_ref());
                Some(hash.into())
            }
            None => None,
        }
    }

    fn insert_data(&mut self, data_hash: H256, code: Bytes) {
        self.db
            .insert_raw(COLUMN_DATA, data_hash.as_slice(), &code)
            .expect("insert data");
    }

    fn get_data(&self, data_hash: &H256) -> Option<Bytes> {
        self.db
            .get(COLUMN_DATA, data_hash.as_slice())
            .map(|slice| Bytes::from(slice.to_vec()))
    }
}
