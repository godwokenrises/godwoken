use anyhow::Result;
use gw_common::H256;
use gw_config::{BackendConfig, BackendType};
use gw_types::bytes::Bytes;
use std::{collections::HashMap, fs};

use crate::AotCode;

#[derive(Clone)]
pub struct Backend {
    pub validator: Bytes,
    pub generator: Bytes,
    pub validator_script_type_hash: H256,
    pub backend_type: BackendType,
}

#[derive(Default)]
pub struct BackendManage {
    backends: HashMap<H256, Backend>,
    /// define here not in backends,
    /// so we don't need to implement the trait `Clone` of AotCode
    #[cfg(feature = "aot")]
    aot_codes: HashMap<H256, AotCode>,
}

impl BackendManage {
    pub fn from_config(configs: Vec<BackendConfig>) -> Result<Self> {
        let mut backend_manage: BackendManage = Default::default();

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
            backend_type,
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
            backend_type,
        };
        self.register_backend(backend);
        Ok(())
    }

    pub fn register_backend(&mut self, backend: Backend) {
        #[cfg(has_asm)]
        #[cfg(feature = "aot")]
        self.aot_codes.insert(
            backend.validator_script_type_hash,
            self.aot_compile(&backend.generator)
                .expect("Ahead-of-time compile"),
        );

        self.backends
            .insert(backend.validator_script_type_hash, backend);
    }

    pub fn get_backend(&self, code_hash: &H256) -> Option<&Backend> {
        self.backends.get(code_hash)
    }

    #[cfg(feature = "aot")]
    fn aot_compile(&self, code_bytes: &Bytes) -> Result<AotCode, ckb_vm::Error> {
        let global_vm_version =
            smol::block_on(async { *gw_ckb_hardfork::GLOBAL_VM_VERSION.lock().await });
        let vm_version = match global_vm_version {
            0 => crate::VMVersion::V0,
            1 => crate::VMVersion::V1,
            ver => panic!("Unsupport VMVersion: {}", ver),
        };
        let mut aot_machine = ckb_vm::machine::aot::AotCompilingMachine::load(
            code_bytes,
            Some(Box::new(crate::vm_cost_model::instruction_cycles)),
            vm_version.vm_isa(),
            vm_version.vm_version(),
        )?;
        aot_machine.compile()
    }

    #[cfg(feature = "aot")]
    pub fn get_aot_code(&self, code_hash: &H256) -> Option<&AotCode> {
        self.aot_codes.get(code_hash)
    }

    #[cfg(not(feature = "aot"))]
    pub(crate) fn get_aot_code(&self, _code_hash: &H256) -> Option<&AotCode> {
        None
    }

    pub fn get_backends(&self) -> &HashMap<H256, Backend> {
        &self.backends
    }
}
