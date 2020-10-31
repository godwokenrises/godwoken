use super::overlay::OverlayState;
use super::wrap_store::WrapStore;
use anyhow::{anyhow, Result};
use gw_common::{
    smt::{Store, H256, SMT},
    sparse_merkle_tree::default_store::DefaultStore,
    state::{Error, State},
};
use parking_lot::Mutex;
use std::sync::Arc;

pub struct StateImpl<S> {
    tree: SMT<WrapStore<S>>,
    account_count: u32,
}

impl<S: Store<H256>> StateImpl<S> {
    pub fn new(root: H256, store: WrapStore<S>, account_count: u32) -> Self {
        let tree = SMT::new(root, store);
        StateImpl {
            tree,
            account_count,
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
}

impl<S: Store<H256> + Default> Default for StateImpl<S> {
    fn default() -> Self {
        let store = WrapStore::new(Arc::new(Mutex::new(S::default())));
        let mut state = StateImpl {
            tree: SMT::new(H256::zero(), store),
            account_count: 0,
        };
        // create a reserve account which id is zero
        let id = state
            .create_account(Default::default(), Default::default())
            .expect("dummy state");
        assert_eq!(id, 0, "reserve id zero");
        state
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
    fn calculate_root(&self) -> Result<[u8; 32], Error> {
        let root = (*self.tree.root()).into();
        Ok(root)
    }
    fn get_account_count(&self) -> Result<u32, Error> {
        Ok(self.account_count)
    }
    fn set_account_count(&mut self, count: u32) -> Result<(), Error> {
        self.account_count = count;
        Ok(())
    }
}
