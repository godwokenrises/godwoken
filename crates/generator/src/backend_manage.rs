use gw_common::{blake2b::new_blake2b, H256};
use gw_types::bytes::Bytes;
use lazy_static::lazy_static;
use std::collections::HashMap;

lazy_static! {
    pub static ref SUDT_GENERATOR: Bytes = include_bytes!("../../../c/build/sudt-generator")
        .to_vec()
        .into();
    // TODO FIXME implement validator
    pub static ref SUDT_VALIDATOR: Bytes = include_bytes!("../../../c/build/sudt-generator")
        .to_vec()
        .into();
    pub static ref SUDT_VALIDATOR_CODE_HASH: H256 = code_hash(&SUDT_VALIDATOR);
}

fn code_hash(data: &[u8]) -> H256 {
    let mut hasher = new_blake2b();
    hasher.update(data);
    let mut code_hash = [0u8; 32];
    hasher.finalize(&mut code_hash);
    code_hash.into()
}

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
