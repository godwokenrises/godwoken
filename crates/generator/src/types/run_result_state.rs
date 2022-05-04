use gw_common::{state::State, H256};
use gw_traits::CodeStore;
use gw_types::{bytes::Bytes, offchain::RunResult};

pub struct RunResultState<'a>(pub &'a mut RunResult);

impl<'a> State for RunResultState<'a> {
    fn get_raw(&self, key: &H256) -> Result<H256, gw_common::error::Error> {
        let v = self.0.read_values.get(key).cloned().unwrap_or_default();
        Ok(v)
    }
    fn update_raw(&mut self, key: H256, value: H256) -> Result<(), gw_common::error::Error> {
        self.0.write.write_values.insert(key, value);
        Ok(())
    }
    fn calculate_root(&self) -> Result<H256, gw_common::error::Error> {
        // unsupported operation
        Err(gw_common::error::Error::InvalidArgs)
    }
    fn get_account_count(&self) -> Result<u32, gw_common::error::Error> {
        self.0
            .write
            .account_count
            .ok_or(gw_common::error::Error::InvalidArgs)
    }
    fn set_account_count(&mut self, count: u32) -> Result<(), gw_common::error::Error> {
        self.0.write.account_count = Some(count);
        Ok(())
    }
}

impl<'a> CodeStore for RunResultState<'a> {
    fn get_data(&self, data_hash: &H256) -> Option<Bytes> {
        self.0.read_data.get(data_hash).cloned()
    }
    fn insert_data(&mut self, data_hash: H256, code: Bytes) {
        self.0.write.write_data.insert(data_hash, code);
    }
    fn get_script(&self, script_hash: &H256) -> Option<gw_types::packed::Script> {
        self.0.get_scripts.get(script_hash).cloned()
    }
    fn insert_script(&mut self, script_hash: H256, script: gw_types::packed::Script) {
        self.0.write.new_scripts.insert(script_hash, script);
    }
}
