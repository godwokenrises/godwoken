use anyhow::Result;
use gw_common::H256;
use gw_config::BackendConfig;
use gw_types::bytes::Bytes;
use std::{collections::HashMap, fs};

#[derive(Debug, Clone, Copy)]
pub enum BackendType {
    Meta,
    Sudt,
    Polyjuice,
}

impl From<&str> for BackendType {
    fn from(backend_type_str: &str) -> Self {
        match backend_type_str {
            "meta" => Self::Meta,
            "sudt" => Self::Sudt,
            "polyjuice" => Self::Polyjuice,
            _ => panic!("Unsupported Backend"),
        }
    }
}

#[derive(Clone)]
pub struct Backend {
    pub backend_type: BackendType,
    pub validator: Bytes,
    pub generator: Bytes,
    pub validator_script_type_hash: H256,
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
            backend_type,
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
            backend_type: BackendType::from(backend_type.as_str()),
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
