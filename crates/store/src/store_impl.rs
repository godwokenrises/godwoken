use super::overlay::{OverlaySMTStore, OverlayStore};
use super::wrap_store::WrapStore;
use anyhow::{anyhow, Result};
use gw_common::{
    error::Error,
    smt::{Store as SMTStore, H256, SMT},
    state::State,
};
use gw_generator::traits::CodeStore;
use gw_types::{
    bytes::Bytes,
    packed::{L2Block, L2Transaction, Script},
};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

pub struct Store<S> {
    tree: SMT<WrapStore<S>>,
    account_count: u32,
    // Note: The block tree can use same storage with the account tree
    // But the column must be difference, otherwise the keys may be collision with each other
    block_tree: SMT<WrapStore<S>>,
    block_count: u64,
    // code store
    scripts: HashMap<H256, Script>,
    blocks: HashMap<H256, L2Block>,
    transactions: HashMap<H256, L2Transaction>,
}

impl<S: SMTStore<H256>> Store<S> {
    pub fn new(
        account_tree: SMT<WrapStore<S>>,
        account_count: u32,
        block_tree: SMT<WrapStore<S>>,
        block_count: u64,
        scripts: HashMap<H256, Script>,
        codes: HashMap<H256, Bytes>,
        blocks: HashMap<H256, L2Block>,
        transactions: HashMap<H256, L2Transaction>,
    ) -> Self {
        Store {
            tree: account_tree,
            account_count,
            block_tree,
            block_count,
            scripts,
            blocks,
            transactions,
        }
    }

    pub fn new_overlay(&self) -> Result<OverlayStore<WrapStore<S>>> {
        let root = self.tree.root();
        let account_count = self
            .get_account_count()
            .map_err(|err| anyhow!("get amount count error: {:?}", err))?;
        let store = OverlaySMTStore::new(self.tree.store().clone());
        Ok(OverlayStore::new(
            *root,
            store,
            account_count,
            self.scripts.clone(),
        ))
    }

    pub fn account_smt(&self) -> &SMT<WrapStore<S>> {
        &self.tree
    }

    pub fn block_smt(&self) -> &SMT<WrapStore<S>> {
        &self.block_tree
    }

    pub fn insert_block(&mut self, block: L2Block) -> Result<()> {
        self.blocks.insert(block.hash().into(), block.clone());
        for tx in block.transactions() {
            self.transactions.insert(tx.hash().into(), tx);
        }
        Ok(())
    }

    /// Attach block to the rollup main chain
    pub fn attach_block(&mut self, block: L2Block) -> Result<()> {
        let raw = block.raw();
        self.block_tree
            .update(raw.smt_key().into(), raw.hash().into())?;
        Ok(())
    }

    pub fn get_block(&self, block_hash: &H256) -> Result<Option<L2Block>, Error> {
        Ok(self.blocks.get(block_hash).cloned())
    }

    pub fn get_transaction(&self, tx_hash: &H256) -> Result<Option<L2Transaction>, Error> {
        Ok(self.transactions.get(tx_hash).cloned())
    }
}

impl<S: SMTStore<H256> + Default> Default for Store<S> {
    fn default() -> Self {
        let tree = SMT::new(
            H256::zero(),
            WrapStore::new(Arc::new(Mutex::new(S::default()))),
        );
        let block_tree = SMT::new(
            H256::zero(),
            WrapStore::new(Arc::new(Mutex::new(S::default()))),
        );
        Store {
            tree,
            account_count: 0,
            block_tree,
            block_count: 0,
            scripts: Default::default(),
            blocks: Default::default(),
            transactions: Default::default(),
        }
    }
}

impl<S: SMTStore<H256>> State for Store<S> {
    fn get_raw(&self, key: &H256) -> Result<H256, Error> {
        let v = self.tree.get(&(*key).into())?;
        Ok(v.into())
    }
    fn update_raw(&mut self, key: H256, value: H256) -> Result<(), Error> {
        self.tree.update(key.into(), value.into())?;
        Ok(())
    }
    fn get_account_count(&self) -> Result<u32, Error> {
        Ok(self.account_count)
    }
    fn set_account_count(&mut self, count: u32) -> Result<(), Error> {
        self.account_count = count;
        Ok(())
    }
    fn calculate_root(&self) -> Result<H256, Error> {
        let root = (*self.tree.root()).into();
        Ok(root)
    }
}

impl<S: SMTStore<H256>> CodeStore for Store<S> {
    fn insert_script(&mut self, script_hash: H256, script: Script) {
        self.scripts.insert(script_hash.into(), script);
    }
    fn get_script(&self, script_hash: &H256) -> Option<Script> {
        self.scripts.get(&script_hash).cloned()
    }
}
