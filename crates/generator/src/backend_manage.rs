use crate::builtin_scripts::{
    META_CONTRACT_GENERATOR, META_CONTRACT_VALIDATOR, META_CONTRACT_VALIDATOR_CODE_HASH,
    SUDT_GENERATOR, SUDT_VALIDATOR, SUDT_VALIDATOR_CODE_HASH,
};
use crate::code_hash;
use gw_common::H256;
use gw_types::bytes::Bytes;
use std::collections::HashMap;

#[derive(Clone)]
pub struct Backend {
    pub validator: Bytes,
    pub generator: Bytes,
    pub validator_code_hash: H256,
}

impl Backend {
    pub fn from_binaries(validator: Bytes, generator: Bytes) -> Backend {
        let validator_code_hash = code_hash(&validator);
        Backend {
            validator,
            generator,
            validator_code_hash,
        }
    }
}

#[derive(Clone)]
pub struct BackendManage {
    backends: HashMap<H256, Backend>,
}

impl Default for BackendManage {
    fn default() -> Self {
        let mut backend_manage = BackendManage {
            backends: Default::default(),
        };

        // Meta contract
        backend_manage.register_backend(Backend {
            validator: META_CONTRACT_VALIDATOR.clone(),
            generator: META_CONTRACT_GENERATOR.clone(),
            validator_code_hash: META_CONTRACT_VALIDATOR_CODE_HASH.clone(),
        });

        // Simple UDT
        backend_manage.register_backend(Backend {
            validator: SUDT_VALIDATOR.clone(),
            generator: SUDT_GENERATOR.clone(),
            validator_code_hash: SUDT_VALIDATOR_CODE_HASH.clone(),
        });

        backend_manage
    }
}

impl BackendManage {
    pub fn register_backend(&mut self, backend: Backend) {
        self.backends.insert(backend.validator_code_hash, backend);
    }

    pub fn get_backend(&self, code_hash: &H256) -> Option<&Backend> {
        self.backends.get(code_hash)
    }
}
