//! A simple wrapper

use gw_common::sparse_merkle_tree::{
    error::Error,
    traits::Store as SMTStore,
    tree::{BranchNode, LeafNode},
    H256,
};
use parking_lot::Mutex;
use std::sync::Arc;

pub struct WrapStore<S> {
    inner: Arc<Mutex<S>>,
}

impl<S> WrapStore<S> {
    pub fn new(inner: Arc<Mutex<S>>) -> Self {
        WrapStore { inner }
    }

    pub fn inner(&self) -> &Mutex<S> {
        &self.inner
    }
}

impl<S> Clone for WrapStore<S> {
    fn clone(&self) -> Self {
        WrapStore::new(Arc::clone(&self.inner))
    }
}

impl<S: SMTStore<H256>> SMTStore<H256> for WrapStore<S> {
    fn get_branch(&self, node: &H256) -> Result<Option<BranchNode>, Error> {
        self.inner.lock().get_branch(node)
    }
    fn get_leaf(&self, leaf_hash: &H256) -> Result<Option<LeafNode<H256>>, Error> {
        self.inner.lock().get_leaf(leaf_hash)
    }
    fn insert_branch(&mut self, node: H256, branch: BranchNode) -> Result<(), Error> {
        self.inner.lock().insert_branch(node, branch)
    }
    fn insert_leaf(&mut self, leaf_hash: H256, leaf: LeafNode<H256>) -> Result<(), Error> {
        self.inner.lock().insert_leaf(leaf_hash, leaf)
    }
    fn remove_branch(&mut self, node: &H256) -> Result<(), Error> {
        self.inner.lock().remove_branch(node)
    }
    fn remove_leaf(&mut self, leaf_hash: &H256) -> Result<(), Error> {
        self.inner.lock().remove_leaf(leaf_hash)
    }
}
