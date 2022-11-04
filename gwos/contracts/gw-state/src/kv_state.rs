use crate::ckb_smt::smt::{Pair, Tree};
use core::cell::RefCell;
use gw_utils::ckb_std::debug;
use gw_utils::error::Error;
use gw_utils::gw_common::{error::Error as SMTError, state::State, H256};
use gw_utils::gw_types::{packed::KVPairVecReader, prelude::*};

pub struct KVState<'a> {
    tree: RefCell<Tree<'a>>,
    proof: &'a [u8],
    account_count: u32,
    previous_root: Option<H256>,
}

impl<'a> KVState<'a> {
    /// params:
    /// - kv_pairs, the kv pairs
    /// - proof, the merkle proof of kv_pairs
    /// - account count, account count in the current state
    /// - current_root, calculate_root returns this value if the kv_paris & proof is empty
    pub fn build(
        buf: &'a mut [Pair],
        kv_pairs: KVPairVecReader,
        proof: &'a [u8],
        account_count: u32,
        current_root: Option<H256>,
    ) -> Result<KVState<'a>, Error> {
        let mut tree = Tree::new(buf);
        for pair in kv_pairs.iter() {
            tree.update(&pair.k().unpack(), &pair.v().unpack())
                .map_err(|err| {
                    debug!("[kv state] build: update key error: {}", err);
                    Error::SMTKeyMissing
                })?;
        }
        Ok(KVState {
            tree: RefCell::new(tree),
            proof,
            account_count,
            previous_root: current_root,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.tree.borrow().is_empty() && self.proof.is_empty()
    }
}

impl<'a> State for KVState<'a> {
    fn get_raw(&self, key: &H256) -> Result<H256, SMTError> {
        // make sure the key must exists in the kv
        Ok(self
            .tree
            .borrow()
            .get(&(*key).into())
            .map_err(|_| SMTError::MissingKey)?
            .into())
    }
    fn update_raw(&mut self, key: H256, value: H256) -> Result<(), SMTError> {
        self.tree
            .borrow_mut()
            .update(&key.into(), &value.into())
            .map_err(|_| SMTError::MissingKey)
    }
    fn get_account_count(&self) -> Result<u32, SMTError> {
        Ok(self.account_count)
    }
    fn set_account_count(&mut self, count: u32) -> Result<(), SMTError> {
        self.account_count = count;
        Ok(())
    }
    fn calculate_root(&self) -> Result<H256, SMTError> {
        if self.is_empty() {
            return self.previous_root.ok_or_else(|| {
                debug!("[kv state] calculate merkle root for an empty kv_state");
                SMTError::MerkleProof
            });
        }
        let mut tree = self.tree.borrow_mut();
        tree.normalize();
        let root = tree.calculate_root(self.proof).map_err(|err| {
            debug!("[kv state] calculate root error: {}", err);
            SMTError::MerkleProof
        })?;
        Ok(root.into())
    }
}
