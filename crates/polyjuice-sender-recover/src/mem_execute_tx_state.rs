use anyhow::Result;
use gw_common::error::Error as StateError;
use gw_common::registry_address::RegistryAddress;
use gw_common::state::State;
use gw_common::H256;
use gw_generator::traits::StateExt;
use gw_traits::CodeStore;
use gw_types::bytes::Bytes;
use gw_types::offchain::RunResult;
use gw_types::packed::Script;

pub struct MemExecuteTxStateTree<S: State> {
    readonly_state: S,
    overlay: RunResult,
}

impl<S: State + CodeStore> MemExecuteTxStateTree<S> {
    pub fn new(readonly_state: S) -> Self {
        Self {
            readonly_state,
            overlay: Default::default(),
        }
    }

    pub fn mock_account(
        &mut self,
        registry_address: RegistryAddress,
        account_script: Script,
    ) -> Result<u32> {
        let account_script_hash: H256 = account_script.hash().into();
        if let Some(account_id) = self
            .readonly_state
            .get_account_id_by_script_hash(&account_script_hash)?
        {
            return Ok(account_id);
        }

        let account_id = self.create_account_from_script(account_script)?;
        self.mapping_registry_address_to_script_hash(registry_address, account_script_hash)?;
        Ok(account_id)
    }
}

impl<S: State + CodeStore> State for MemExecuteTxStateTree<S> {
    fn get_raw(&self, key: &H256) -> Result<H256, StateError> {
        match self.overlay.write.write_values.get(key) {
            Some(value) => Ok(*value),
            None => self.readonly_state.get_raw(key),
        }
    }

    fn update_raw(&mut self, key: H256, value: H256) -> Result<(), StateError> {
        self.overlay.write.write_values.insert(key, value);
        Ok(())
    }

    fn get_account_count(&self) -> Result<u32, StateError> {
        match self.overlay.write.account_count {
            Some(count) => Ok(count),
            None => self.readonly_state.get_account_count(),
        }
    }

    fn set_account_count(&mut self, count: u32) -> Result<(), StateError> {
        self.overlay.write.account_count = Some(count);
        Ok(())
    }

    fn finalise_root(&mut self) -> Result<H256, StateError> {
        log::error!("calculate_root is unsupport in executetx state");
        Err(StateError::Store)
    }
}

impl<S: State + CodeStore> CodeStore for MemExecuteTxStateTree<S> {
    fn insert_script(&mut self, script_hash: H256, script: Script) {
        self.overlay.write.new_scripts.insert(script_hash, script);
    }

    fn get_script(&self, script_hash: &H256) -> Option<Script> {
        match self.overlay.write.new_scripts.get(script_hash) {
            Some(script) => Some(script.to_owned()),
            None => self.readonly_state.get_script(script_hash),
        }
    }

    fn insert_data(&mut self, data_hash: H256, code: Bytes) {
        self.overlay.write.write_data.insert(data_hash, code);
    }

    fn get_data(&self, data_hash: &H256) -> Option<Bytes> {
        match self.overlay.write.write_data.get(data_hash) {
            Some(data) => Some(data.to_owned()),
            None => self.readonly_state.get_data(data_hash),
        }
    }
}
