use anyhow::Result;
use gw_common::H256;
use gw_config::{BackendConfig, BackendManageConfig};
use gw_types::bytes::Bytes;
use std::{collections::HashMap, fs};

#[derive(Clone)]
pub struct Backend {
    pub validator: Bytes,
    pub generator: Bytes,
    pub validator_script_type_hash: H256,
}

#[derive(Clone)]
pub struct BackendManage {
    backends: HashMap<H256, Backend>,
}

impl BackendManage {
    pub fn from_config(config: BackendManageConfig) -> Result<Self> {
        let mut backend_manage = BackendManage {
            backends: Default::default(),
        };

        let BackendManageConfig {
            meta_contract,
            simple_udt,
        } = config;

        // Meta contract
        backend_manage.register_backend_config(meta_contract)?;
        // Simple UDT
        backend_manage.register_backend_config(simple_udt)?;

        Ok(backend_manage)
    }

    pub fn register_backend_config(&mut self, config: BackendConfig) -> Result<()> {
        let BackendConfig {
            validator_path,
            generator_path,
            validator_script_type_hash,
        } = config;
        let validator = fs::read(validator_path)?.into();
        let generator = fs::read(generator_path)?.into();
        let validator_script_type_hash = {
            let hash: [u8; 32] = validator_script_type_hash.into();
            hash.into()
        };
        let backend = Backend {
            validator,
            generator,
            validator_script_type_hash,
        };
        self.register_backend(backend);
        Ok(())
    }

    pub fn register_backend(&mut self, backend: Backend) {
        self.backends
            .insert(backend.validator_script_type_hash, backend);
    }

    pub fn get_backend(&self, code_hash: &H256) -> Option<&Backend> {
        self.backends.get(code_hash)
    }
}
