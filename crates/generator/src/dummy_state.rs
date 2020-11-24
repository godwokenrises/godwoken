use gw_common::{
    smt::{default_store::DefaultStore, H256, SMT},
    state::{Error, State},
};

pub struct DummyState {
    tree: SMT<DefaultStore<H256>>,
    account_count: u32,
    sudt_account_id: u32,
}

impl Default for DummyState {
    fn default() -> Self {
        let mut state = DummyState {
            tree: Default::default(),
            account_count: 0,
            sudt_account_id: 1,
        };
        state
    }
}

impl State for DummyState {
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
