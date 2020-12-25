use crate::genesis::GenesisWithSMTState;

use super::overlay::{OverlaySMTStore, OverlayStore};
use super::wrap_store::WrapStore;
use anyhow::{anyhow, Result};
use gw_common::{
    error::Error,
    smt::{Store as SMTStore, H256, SMT},
    state::State,
};
use gw_generator::{traits::CodeStore, TxReceipt};
use gw_types::{
    bytes::Bytes,
    packed::{GlobalState, HeaderInfo, L2Block, L2Transaction, Script},
    prelude::*,
};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

pub struct Store<S> {
    account_tree: SMT<WrapStore<S>>,
    account_count: u32,
    // Note: The block tree can use same storage with the account tree
    // But the column must be difference, otherwise the keys may be collision with each other
    block_tree: SMT<WrapStore<S>>,
    // code store
    scripts: HashMap<H256, Script>,
    codes: HashMap<H256, Bytes>,
    blocks: HashMap<H256, L2Block>,
    header_infos: HashMap<H256, HeaderInfo>,
    tip_block_hash: H256,
    tip_block_number: u64,
    tip_global_state: GlobalState,
    transactions: HashMap<H256, (L2Transaction, TxReceipt)>,
}

impl<S: SMTStore<H256>> Store<S> {
    pub fn new(
        account_tree: SMT<WrapStore<S>>,
        account_count: u32,
        block_tree: SMT<WrapStore<S>>,
        scripts: HashMap<H256, Script>,
        tip_block_hash: H256,
        tip_block_number: u64,
        tip_global_state: GlobalState,
        blocks: HashMap<H256, L2Block>,
        header_infos: HashMap<H256, HeaderInfo>,
        codes: HashMap<H256, Bytes>,
        transactions: HashMap<H256, (L2Transaction, TxReceipt)>,
    ) -> Self {
        Store {
            account_tree,
            account_count,
            block_tree,
            scripts,
            codes,
            blocks,
            header_infos,
            tip_block_hash,
            tip_block_number,
            tip_global_state,
            transactions,
        }
    }

    pub fn init_genesis(
        &mut self,
        genesis_with_smt: GenesisWithSMTState,
        header_info: HeaderInfo,
    ) -> Result<()> {
        let GenesisWithSMTState {
            genesis,
            leaves_map,
            branches_map,
            global_state,
        } = genesis_with_smt;

        // initialize account smt
        {
            let smt_store = self.account_tree.store_mut();
            for (leaf_hash, leaf) in leaves_map {
                smt_store.insert_leaf(leaf_hash, leaf)?;
            }
            for (node, branch) in branches_map {
                smt_store.insert_branch(node, branch)?;
            }
        }
        assert!(
            genesis.transactions().is_empty(),
            "assume genesis has no txs"
        );
        self.insert_block(genesis.clone(), header_info, Vec::new())?;
        self.attach_block(genesis)?;
        self.set_tip_global_state(global_state)?;
        Ok(())
    }

    pub fn new_overlay(&self) -> Result<OverlayStore<WrapStore<S>>> {
        let root = self.account_tree.root();
        let account_count = self
            .get_account_count()
            .map_err(|err| anyhow!("get amount count error: {:?}", err))?;
        let store = OverlaySMTStore::new(self.account_tree.store().clone());
        Ok(OverlayStore::new(
            *root,
            store,
            account_count,
            self.scripts.clone(),
            self.codes.clone(),
        ))
    }

    pub fn account_smt(&self) -> &SMT<WrapStore<S>> {
        &self.account_tree
    }

    pub fn block_smt(&self) -> &SMT<WrapStore<S>> {
        &self.block_tree
    }

    pub fn insert_block(
        &mut self,
        block: L2Block,
        header_info: HeaderInfo,
        tx_receipts: Vec<TxReceipt>,
    ) -> Result<()> {
        let block_hash = block.hash().into();
        self.blocks.insert(block_hash, block.clone());
        self.header_infos.insert(block_hash, header_info);
        debug_assert_eq!(block.transactions().len(), tx_receipts.len());
        for (tx, tx_receipt) in block.transactions().into_iter().zip(tx_receipts) {
            self.transactions.insert(tx.hash().into(), (tx, tx_receipt));
        }
        Ok(())
    }

    /// Attach block to the rollup main chain
    pub fn attach_block(&mut self, block: L2Block) -> Result<()> {
        let raw = block.raw();
        let block_number: u64 = raw.number().unpack();
        if block_number != 0 {
            assert_eq!(self.tip_block_number + 1, block_number);
        }
        self.block_tree
            .update(raw.smt_key().into(), raw.hash().into())?;
        self.tip_block_hash = raw.hash().into();
        self.tip_block_number = block_number;
        Ok(())
    }

    pub fn get_tip_block(&self) -> Result<Option<L2Block>, Error> {
        self.get_block(&self.tip_block_hash)
    }

    pub fn get_tip_global_state(&self) -> Result<GlobalState, Error> {
        Ok(self.tip_global_state.clone())
    }

    pub fn set_tip_global_state(&mut self, global_state: GlobalState) -> Result<(), Error> {
        self.tip_global_state = global_state;
        Ok(())
    }

    pub fn get_block(&self, block_hash: &H256) -> Result<Option<L2Block>, Error> {
        Ok(self.blocks.get(block_hash).cloned())
    }

    pub fn get_block_synced_header_info(
        &self,
        block_hash: &H256,
    ) -> Result<Option<HeaderInfo>, Error> {
        Ok(self.header_infos.get(block_hash).cloned())
    }

    pub fn get_transaction(&self, tx_hash: &H256) -> Result<Option<L2Transaction>, Error> {
        Ok(self.transactions.get(tx_hash).map(|(tx, _)| tx).cloned())
    }

    pub fn get_transaction_receipt(&self, tx_hash: &H256) -> Result<Option<TxReceipt>, Error> {
        Ok(self
            .transactions
            .get(tx_hash)
            .map(|(_, tx_receipt)| tx_receipt)
            .cloned())
    }
}

impl<S: SMTStore<H256> + Default> Default for Store<S> {
    fn default() -> Self {
        let account_tree = SMT::new(
            H256::zero(),
            WrapStore::new(Arc::new(Mutex::new(S::default()))),
        );
        let block_tree = SMT::new(
            H256::zero(),
            WrapStore::new(Arc::new(Mutex::new(S::default()))),
        );
        Store {
            account_tree,
            account_count: 0,
            block_tree,
            scripts: Default::default(),
            codes: Default::default(),
            blocks: Default::default(),
            header_infos: Default::default(),
            tip_block_hash: H256::zero(),
            tip_block_number: 0,
            tip_global_state: Default::default(),
            transactions: Default::default(),
        }
    }
}

impl<S: SMTStore<H256>> State for Store<S> {
    fn get_raw(&self, key: &H256) -> Result<H256, Error> {
        let v = self.account_tree.get(&(*key).into())?;
        Ok(v.into())
    }
    fn update_raw(&mut self, key: H256, value: H256) -> Result<(), Error> {
        self.account_tree.update(key.into(), value.into())?;
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
        let root = (*self.account_tree.root()).into();
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
    fn insert_data(&mut self, script_hash: H256, code: Bytes) {
        self.codes.insert(script_hash, code);
    }
    fn get_data(&self, script_hash: &H256) -> Option<Bytes> {
        self.codes.get(script_hash).cloned()
    }
}
