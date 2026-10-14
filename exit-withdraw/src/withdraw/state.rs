//! Ephemeral copy-on-write SMT over the immutable audited DB. No DB writes.
use crate::db::Database;
use anyhow::{ensure, Result};
use exit_withdraw_state_validator as contract;
use gw_smt::{
    smt::{SMT, SMTH256},
    sparse_merkle_tree::{
        error::Error,
        traits::{StoreReadOps, StoreWriteOps},
        BranchKey, BranchNode,
    },
};
use std::collections::BTreeMap;

pub struct Overlay<'a> {
    base: &'a dyn StoreReadOps<SMTH256>,
    branches: BTreeMap<BranchKey, Option<BranchNode>>,
    leaves: BTreeMap<SMTH256, Option<SMTH256>>,
}
impl StoreReadOps<SMTH256> for Overlay<'_> {
    fn get_branch(&self, k: &BranchKey) -> Result<Option<BranchNode>, Error> {
        match self.branches.get(k) {
            Some(v) => Ok(v.clone()),
            None => self.base.get_branch(k),
        }
    }
    fn get_leaf(&self, k: &SMTH256) -> Result<Option<SMTH256>, Error> {
        match self.leaves.get(k) {
            Some(v) => Ok(*v),
            None => self.base.get_leaf(k),
        }
    }
}
impl StoreWriteOps<SMTH256> for Overlay<'_> {
    fn insert_branch(&mut self, k: BranchKey, v: BranchNode) -> Result<(), Error> {
        self.branches.insert(k, Some(v));
        Ok(())
    }
    fn remove_branch(&mut self, k: &BranchKey) -> Result<(), Error> {
        self.branches.insert(k.clone(), None);
        Ok(())
    }
    fn insert_leaf(&mut self, k: SMTH256, v: SMTH256) -> Result<(), Error> {
        self.leaves.insert(k, Some(v));
        Ok(())
    }
    fn remove_leaf(&mut self, k: &SMTH256) -> Result<(), Error> {
        self.leaves.insert(*k, None);
        Ok(())
    }
}
pub struct RecoveryState<'a> {
    pub tree: SMT<Overlay<'a>>,
}
impl<'a> RecoveryState<'a> {
    pub fn new(db: &'a Database) -> Result<Self> {
        ensure!(
            db.root() == db.tip.account_root,
            "Original DB root differs from its last-valid block"
        );
        Ok(Self::from_store(*db.tree.root(), db.tree.store()))
    }
    pub fn from_store(root: SMTH256, base: &'a dyn StoreReadOps<SMTH256>) -> Self {
        Self {
            tree: SMT::new(
                root,
                Overlay {
                    base,
                    branches: BTreeMap::new(),
                    leaves: BTreeMap::new(),
                },
            ),
        }
    }
    pub fn root(&self) -> contract::Hash {
        (*self.tree.root()).into()
    }
    pub fn read(&self, key: contract::Hash) -> Result<contract::Hash> {
        let k = key.into();
        let v = self.tree.store().get_leaf(&k)?.unwrap_or_default();
        ensure!(
            self.tree
                .merkle_proof(vec![k])?
                .verify::<gw_smt::smt::Blake2bHasher>(self.tree.root(), vec![(k, v)])?,
            "Corrupt state leaf"
        );
        Ok(v.into())
    }
    pub fn apply(&mut self, a: &contract::Action, outcome: &contract::Outcome) -> Result<()> {
        for ((r, id), debit) in &outcome.debits {
            let key = contract::balance_key(*id, a.recipients[*r as usize].address);
            let n = contract::amount(self.read(key)?)
                .checked_sub(*debit)
                .ok_or_else(|| anyhow::anyhow!("Balance underflow"))?;
            self.tree.update(key.into(), contract::value(n).into())?;
            let key = contract::supply_key(*id);
            let n = contract::amount(self.read(key)?)
                .checked_sub(*debit)
                .ok_or_else(|| anyhow::anyhow!("Supply underflow"))?;
            self.tree.update(key.into(), contract::value(n).into())?;
        }
        ensure!(self.root() == outcome.root, "Replayed state root mismatch");
        Ok(())
    }
}
