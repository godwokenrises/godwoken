use anyhow::Result;
use gw_common::H256;
use gw_config::BackendConfig;
use gw_types::bytes::Bytes;
use std::{collections::HashMap, fs};

#[derive(Debug, Clone, Copy)]
pub enum BackendName {
    Meta,
    Sudt,
    Polyjuice,
}

#[derive(Clone)]
pub struct Backend {
    pub name: Option<BackendName>,
    pub validator: Bytes,
    pub generator: Bytes,
    pub validator_script_type_hash: H256,
}

#[derive(Clone)]
pub struct BackendManage {
    backends: HashMap<H256, Backend>,
}

/// Get backend name from the validator path
fn get_backend_name(validator_path: &str) -> Option<BackendName> {
    if validator_path.contains("meta") {
        Some(BackendName::Meta)
    } else if validator_path.contains("sudt") {
        Some(BackendName::Sudt)
    } else if validator_path.contains("polyjuice") {
        Some(BackendName::Polyjuice)
    } else {
        None
    }
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
        let name = get_backend_name(validator_path.to_str().unwrap());
        let validator = fs::read(validator_path)?.into();
        let generator = fs::read(generator_path)?.into();
        let validator_script_type_hash = {
            let hash: [u8; 32] = validator_script_type_hash.into();
            hash.into()
        };
        let backend = Backend {
            name,
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

    pub fn get_backends(&self) -> &HashMap<H256, Backend> {
        &self.backends
    }
}
