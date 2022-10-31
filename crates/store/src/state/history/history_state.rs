//! State DB

use anyhow::Result;
use gw_common::{error::Error as StateError, smt::SMT, state::State, H256};
use gw_traits::CodeStore;
use gw_types::{
    bytes::Bytes,
    packed::{self, AccountMerkleState, Byte32},
    prelude::*,
};
use log::log_enabled;

use crate::{
    smt::smt_store::SMTStateStore, state::history::block_state_record::BlockStateRecordKey,
    traits::kv_store::KVStore,
};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ReadOpt {
    Block(u64),
    Any,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum WriteOpt {
    NoRecord,
    Block(u64),
    Deny,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct RWConfig {
    read: ReadOpt,
    write: WriteOpt,
}

impl RWConfig {
    pub fn readonly() -> Self {
        RWConfig {
            read: ReadOpt::Any,
            write: WriteOpt::Deny,
        }
    }

    pub fn attach_block(number: u64) -> Self {
        RWConfig {
            read: ReadOpt::Any,
            write: WriteOpt::Block(number),
        }
    }

    pub fn detach_block() -> Self {
        RWConfig {
            read: ReadOpt::Any,
            write: WriteOpt::NoRecord,
        }
    }

    pub fn history_block(number: u64) -> Self {
        RWConfig {
            read: ReadOpt::Block(number),
            write: WriteOpt::Deny,
        }
    }
}

pub trait HistoryStateStore {
    type BlockStateRecordKeyIter: IntoIterator<Item = BlockStateRecordKey>;
    fn iter_block_state_record(&self, block_number: u64) -> Self::BlockStateRecordKeyIter;
    fn remove_block_state_record(&self, block_number: u64) -> Result<(), anyhow::Error>;
    fn get_history_state(&self, block_number: u64, state_key: &H256) -> Option<H256>;
    fn record_block_state(
        &self,
        block_number: u64,
        state_key: H256,
        value: H256,
    ) -> Result<(), anyhow::Error>;
}

pub struct HistoryState<TreeStore> {
    account_count: u32,
    tree: SMT<SMTStateStore<TreeStore>>,
    rw_config: RWConfig,
}

impl<Store: Clone + KVStore> Clone for HistoryState<Store> {
    fn clone(&self) -> Self {
        Self {
            account_count: self.account_count,
            tree: SMT::new(*self.tree.root(), self.tree.store().clone()),
            rw_config: self.rw_config,
        }
    }
}

impl<Store: HistoryStateStore + CodeStore + KVStore> HistoryState<Store> {
    pub fn new(tree: SMT<SMTStateStore<Store>>, account_count: u32, rw_config: RWConfig) -> Self {
        Self {
            tree,
            account_count,
            rw_config,
        }
    }

    pub fn get_merkle_state(&self) -> AccountMerkleState {
        AccountMerkleState::new_builder()
            .merkle_root(self.tree.root().pack())
            .count(self.account_count.pack())
            .build()
    }

    /// Detach block state from state tree
    pub fn detach_block_state(&mut self, block_number: u64) -> Result<()> {
        // reset states to previous value
        let parent_block_number = block_number.saturating_sub(1);
        let reverted_key_values: Vec<_> = self
            .db()
            .iter_block_state_record(block_number)
            .into_iter()
            .map(|record_key| {
                let state_key = record_key.state_key();
                let last_value = self
                    .db()
                    .get_history_state(parent_block_number, &state_key)
                    .unwrap_or_else(H256::zero);
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

    fn db(&self) -> &Store {
        self.tree.store().inner_store()
    }

    fn db_mut(&mut self) -> &mut Store {
        self.tree.store_mut().inner_store_mut()
    }
}

impl<Store: KVStore + HistoryStateStore + CodeStore> State for HistoryState<Store> {
    fn get_raw(&self, key: &H256) -> Result<H256, StateError> {
        let v = match self.rw_config.read {
            ReadOpt::Block(block_number) => self
                .db()
                .get_history_state(block_number, key)
                .unwrap_or_default(),
            _ => self.tree.get(key)?,
        };
        if log_enabled!(log::Level::Trace) {
            let k: Byte32 = key.pack();
            let v: Byte32 = v.pack();
            log::trace!(
                "[state-trace] get_raw rw_config:{:?} k:{} v:{}",
                self.rw_config,
                k,
                v
            );
        }
        Ok(v)
    }

    fn update_raw(&mut self, key: H256, value: H256) -> Result<(), StateError> {
        self.tree.update(key, value)?;
        // record block's kv state
        match self.rw_config.write {
            WriteOpt::Block(block_number) => {
                self.db()
                    .record_block_state(block_number, key, value)
                    .expect("record block state");
            }
            WriteOpt::NoRecord => {
                // skip record history state, dettach block may use this config
            }
            WriteOpt::Deny => {
                // deny write
                return Err(StateError::Store);
            }
        }
        if log_enabled!(log::Level::Trace) {
            let k: Byte32 = key.pack();
            let v: Byte32 = value.pack();
            log::trace!(
                "[state-trace] update_raw rw_config:{:?} k:{} v:{}",
                self.rw_config,
                k,
                v
            );
        }
        Ok(())
    }

    fn get_account_count(&self) -> Result<u32, StateError> {
        if log_enabled!(log::Level::Trace) {
            log::trace!(
                "[state-trace] get_account_count rw_config:{:?} count:{}",
                self.rw_config,
                self.account_count
            );
        }
        Ok(self.account_count)
    }

    fn set_account_count(&mut self, count: u32) -> Result<(), StateError> {
        if log_enabled!(log::Level::Trace) {
            log::trace!(
                "[state-trace] set_account_count rw_config:{:?} origin: {} count:{}",
                self.rw_config,
                self.account_count,
                count
            );
        }
        self.account_count = count;
        Ok(())
    }

    fn finalise_root(&mut self) -> Result<H256, StateError> {
        let root = self.tree.root();
        Ok(*root)
    }
}

/// TODO Store scripts and data by block height
impl<Store: KVStore + HistoryStateStore + CodeStore> CodeStore for HistoryState<Store> {
    fn insert_script(&mut self, script_hash: H256, script: packed::Script) {
        self.db_mut().insert_script(script_hash, script)
    }

    fn get_script(&self, script_hash: &H256) -> Option<packed::Script> {
        self.db().get_script(script_hash)
    }

    fn insert_data(&mut self, data_hash: H256, code: Bytes) {
        self.db_mut().insert_data(data_hash, code)
    }

    fn get_data(&self, data_hash: &H256) -> Option<Bytes> {
        self.db().get_data(data_hash)
    }
}
