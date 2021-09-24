use anyhow::Result;
use gw_common::{backend::BackendInfo, blake2b::new_blake2b, H256};
use gw_config::BackendConfig;
use gw_types::bytes::Bytes;
use std::{collections::HashMap, fs};

#[derive(Clone)]
pub struct Backend {
    pub validator: Bytes,
    pub generator: Bytes,
    pub validator_script_type_hash: H256,
}

impl Backend {
    fn get_backend_info(&self) -> BackendInfo {
        let mut validator_code_hash = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&self.validator);
        hasher.finalize(&mut validator_code_hash);
        let mut generator_code_hash = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&self.generator);
        hasher.finalize(&mut generator_code_hash);
        BackendInfo {
            validator_code_hash: validator_code_hash.into(),
            generator_code_hash: generator_code_hash.into(),
            validator_script_type_hash: self.validator_script_type_hash,
        }
    }
}

#[derive(Clone)]
pub struct BackendManage {
    backends: HashMap<H256, Backend>,
}

impl BackendManage {
    pub fn from_config(configs: Vec<BackendConfig>) -> Result<Self> {
        let mut backend_manage = BackendManage {
            backends: Default::default(),
        };

        for config in configs {
            backend_manage.register_backend_config(config)?;
        }

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

    pub fn get_backend_info(&self) -> Vec<BackendInfo> {
        self.backends
            .values()
            .into_iter()
            .map(|backend| backend.get_backend_info())
            .collect()
    }
}
