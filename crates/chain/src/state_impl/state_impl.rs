use super::overlay::OverlayState;
use super::wrap_store::WrapStore;
use anyhow::{anyhow, Result};
use gw_common::{
    smt::{CompiledMerkleProof, Store, H256, SMT},
    state::{Error, State},
};
use gw_types::{
    packed::{L2Block, L2Transaction, RawL2Block},
    prelude::*,
};
use parking_lot::Mutex;
use std::sync::Arc;

pub struct StateImpl<S> {
    tree: SMT<WrapStore<S>>,
    account_count: u32,
    // Note: The block tree can use same storage with the account tree
    // But the column must be difference, otherwise the keys may be collision with each other
    block_tree: SMT<WrapStore<S>>,
    block_count: u64,
}

impl<S: Store<H256>> StateImpl<S> {
    pub fn new(
        account_tree: SMT<WrapStore<S>>,
        account_count: u32,
        block_tree: SMT<WrapStore<S>>,
        block_count: u64,
    ) -> Self {
        StateImpl {
            tree: account_tree,
            account_count,
            block_tree,
            block_count,
        }
    }

    pub fn new_overlay(&self) -> Result<OverlayState<WrapStore<S>>> {
        let root = self.tree.root();
        let account_count = self
            .get_account_count()
            .map_err(|err| anyhow!("get amount count error: {:?}", err))?;
        let store = self.tree.store().clone();
        Ok(OverlayState::new(*root, store, account_count))
    }

    pub fn merkle_proof(&self, leaves: Vec<([u8; 32], [u8; 32])>) -> Result<Vec<u8>, Error> {
        let keys = leaves.iter().map(|(k, v)| (*k).into()).collect();
        let proof = self
            .tree
            .merkle_proof(keys)?
            .compile(
                leaves
                    .into_iter()
                    .map(|(k, v)| (k.into(), v.into()))
                    .collect(),
            )?
            .0;
        Ok(proof)
    }

    pub fn push_block(&mut self, block: L2Block) -> Result<()> {
        let raw = block.raw();
        let block_hash = raw.hash();
        let block_number = raw.number().unpack();
        let key = raw.smt_key();
        self.block_tree.update(key.into(), block_hash.into())?;
        Ok(())
    }

    pub fn block_merkle_proof(&self, number: u64) -> Result<CompiledMerkleProof, Error> {
        let key = RawL2Block::compute_smt_key(number);
        let value = self.block_tree.get(&key.into())?;
        let proof = self
            .block_tree
            .merkle_proof(vec![key.into()])?
            .compile(vec![(key.into(), value.into())])?;
        Ok(proof)
    }

    pub fn get_block(&self, number: u64) -> Result<L2Block, Error> {
        unimplemented!()
    }
    pub fn get_block_hash(&self, number: u64) -> Result<H256, Error> {
        unimplemented!()
    }
    pub fn get_transaction(&self, tx_hash: &H256) -> Result<L2Transaction, Error> {
        unimplemented!()
    }
}

impl<S: Store<H256> + Default> Default for StateImpl<S> {
    fn default() -> Self {
        let tree = SMT::new(
            H256::zero(),
            WrapStore::new(Arc::new(Mutex::new(S::default()))),
        );
        let block_tree = SMT::new(
            H256::zero(),
            WrapStore::new(Arc::new(Mutex::new(S::default()))),
        );
        StateImpl {
            tree,
            account_count: 0,
            block_tree,
            block_count: 0,
        }
    }
}

impl<S: Store<H256>> State for StateImpl<S> {
    fn get_raw(&self, key: &[u8; 32]) -> Result<[u8; 32], Error> {
        let v = self.tree.get(&(*key).into())?;
        Ok(v.into())
    }
    fn update_raw(&mut self, key: [u8; 32], value: [u8; 32]) -> Result<(), Error> {
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
    fn calculate_root(&self) -> Result<[u8; 32], Error> {
        let root = (*self.tree.root()).into();
        Ok(root)
    }
}
