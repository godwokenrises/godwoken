use anyhow::{bail, Context, Result};
use gw_common::{blake2b::new_blake2b, H256};
use gw_config::{BackendConfig, BackendForkConfig, BackendType};
use gw_types::bytes::Bytes;
use std::{collections::HashMap, fs};

#[cfg(has_asm)]
use crate::types::vm::AotCode;

#[derive(Default, Clone)]
pub struct BackendCheckSum {
    pub validator: H256,
    pub generator: H256,
}

impl std::fmt::Debug for BackendCheckSum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackendCheckSum")
            .field("validator", &hex::encode(self.validator.as_slice()))
            .field("generator", &hex::encode(self.generator.as_slice()))
            .finish()
    }
}

#[derive(Clone)]
pub struct Backend {
    pub validator: Bytes,
    pub generator: Bytes,
    pub validator_script_type_hash: H256,
    pub backend_type: BackendType,
    pub checksum: BackendCheckSum,
}

impl Backend {
    pub fn new(
        backend_type: BackendType,
        validator_script_type_hash: H256,
        validator: Bytes,
        generator: Bytes,
    ) -> Self {
        let checksum = {
            let validator = {
                let mut hasher = new_blake2b();
                hasher.update(&validator);
                let mut buf = [0u8; 32];
                hasher.finalize(&mut buf);
                buf.into()
            };
            let generator = {
                let mut hasher = new_blake2b();
                hasher.update(&generator);
                let mut buf = [0u8; 32];
                hasher.finalize(&mut buf);
                buf.into()
            };

            BackendCheckSum {
                validator,
                generator,
            }
        };

        Self {
            validator,
            generator,
            validator_script_type_hash,
            backend_type,
            checksum,
        }
    }
}

#[derive(Default)]
pub struct BackendManage {
    backend_forks: Vec<(u64, HashMap<H256, Backend>)>,
    /// define here not in backends,
    /// so we don't need to implement the trait `Clone` of AotCode
    #[cfg(has_asm)]
    aot_codes: HashMap<H256, AotCode>,
}

impl BackendManage {
    pub fn from_config(configs: Vec<BackendForkConfig>) -> Result<Self> {
        let mut backend_manage: BackendManage = Default::default();
        for config in configs {
            backend_manage.register_backend_fork(config, true)?;
        }

        Ok(backend_manage)
    }

    pub fn register_backend_fork(
        &mut self,
        config: BackendForkConfig,
        #[allow(unused_variables)] compile: bool,
    ) -> Result<()> {
        if let Some((height, _backends)) = self.backend_forks.last() {
            if config.fork_height <= *height {
                bail!("BackendForkConfig with fork_height {} is less or equals to the last fork_height {}", config.fork_height, height);
            }
        }
        // inherit backends
        let mut backends = self
            .backend_forks
            .last()
            .map(|(_height, backends)| backends)
            .cloned()
            .unwrap_or_default();

        let fork_height = config.fork_height;

        // register backends
        for config in config.backends {
            let BackendConfig {
                validator_path,
                generator_path,
                validator_script_type_hash,
                backend_type,
            } = config;
            let validator = fs::read(&validator_path)
                .with_context(|| {
                    format!("load validator from {}", validator_path.to_string_lossy())
                })?
                .into();
            let generator = fs::read(&generator_path)
                .with_context(|| {
                    format!("load generator from {}", generator_path.to_string_lossy())
                })?
                .into();
            let validator_script_type_hash = {
                let hash: [u8; 32] = validator_script_type_hash.into();
                hash.into()
            };
            let backend = Backend::new(
                backend_type,
                validator_script_type_hash,
                validator,
                generator,
            );
            #[cfg(has_asm)]
            if compile {
                self.compile_backend(&backend);
            }

            log::debug!(
                "registry backend {:?}({:?}) at height {}",
                backend.backend_type,
                backend.checksum,
                fork_height
            );

            backends.insert(backend.validator_script_type_hash, backend);
        }

        self.backend_forks.push((config.fork_height, backends));
        Ok(())
    }

    #[cfg(has_asm)]
    fn compile_backend(&mut self, backend: &Backend) {
        self.aot_codes.insert(
            backend.checksum.generator,
            self.aot_compile(&backend.generator)
                .expect("Ahead-of-time compile"),
        );
    }

    pub fn get_backends_at_height(
        &self,
        block_number: u64,
    ) -> Option<&(u64, HashMap<H256, Backend>)> {
        self.backend_forks
            .iter()
            .rev()
            .find(|(height, _)| block_number >= *height)
    }

    pub fn get_backend(&self, block_number: u64, code_hash: &H256) -> Option<&Backend> {
        self.get_backends_at_height(block_number)
            .and_then(|(_number, backends)| backends.get(code_hash))
            .map(|backend| {
                log::debug!(
                    "get backend {:?}({:?}) at height {}",
                    backend.backend_type,
                    backend.checksum,
                    block_number
                );
                backend
            })
    }

    #[cfg(has_asm)]
    fn aot_compile(&self, code_bytes: &Bytes) -> Result<AotCode, ckb_vm::Error> {
        let vm_version = crate::types::vm::VMVersion::V1;
        let mut aot_machine = ckb_vm::machine::aot::AotCompilingMachine::load(
            code_bytes,
            Some(Box::new(crate::vm_cost_model::instruction_cycles)),
            vm_version.vm_isa(),
            vm_version.vm_version(),
        )?;
        aot_machine.compile()
    }

    /// get aot_code according to special VM version
    #[cfg(has_asm)]
    pub(crate) fn get_aot_code(&self, code_hash: &H256) -> Option<&AotCode> {
        log::debug!("get_aot_code hash: {}", hex::encode(code_hash.as_slice()),);
        self.aot_codes.get(code_hash)
    }
}

#[cfg(test)]
mod tests {
    use gw_config::{BackendConfig, BackendForkConfig, BackendType};

    use super::BackendManage;

    #[test]
    fn test_get_backend() {
        let mut m = BackendManage::default();
        // prepare fake binaries
        let dir = tempfile::tempdir().unwrap().into_path();
        let sudt_v0 = dir.join("sudt_v0");
        let sudt_v1 = dir.join("sudt_v1");
        let meta_v0 = dir.join("meta_v0");
        let addr_v0 = dir.join("addr_v0");
        std::fs::write(&sudt_v0, "sudt_v0").unwrap();
        std::fs::write(&sudt_v1, "sudt_v1").unwrap();
        std::fs::write(&meta_v0, "meta_v0").unwrap();
        std::fs::write(&addr_v0, "addr_v0").unwrap();

        let config = BackendForkConfig {
            fork_height: 1,
            backends: vec![
                BackendConfig {
                    validator_script_type_hash: [42u8; 32].into(),
                    backend_type: BackendType::Sudt,
                    generator_path: format!("{}/sudt_v0", dir.to_string_lossy()).into(),
                    validator_path: format!("{}/sudt_v0", dir.to_string_lossy()).into(),
                },
                BackendConfig {
                    validator_script_type_hash: [43u8; 32].into(),
                    backend_type: BackendType::EthAddrReg,
                    generator_path: format!("{}/addr_v0", dir.to_string_lossy()).into(),
                    validator_path: format!("{}/addr_v0", dir.to_string_lossy()).into(),
                },
            ],
        };
        m.register_backend_fork(config, false).unwrap();
        assert!(m.get_backends_at_height(0).is_none(), "no backends at 0");
        assert!(
            m.get_backend(1, &[42u8; 32].into()).is_some(),
            "get backend at 1"
        );
        assert!(
            m.get_backend(100, &[42u8; 32].into()).is_some(),
            "get backend at 100"
        );
        assert!(
            m.get_backend(0, &[43u8; 32].into()).is_none(),
            "get backend at 0"
        );
        assert!(
            m.get_backend(1, &[43u8; 32].into()).is_some(),
            "get backend at 1"
        );
        assert!(
            m.get_backend(100, &[43u8; 32].into()).is_some(),
            "get backend at 100"
        );

        let config = BackendForkConfig {
            fork_height: 5,
            backends: vec![
                BackendConfig {
                    validator_script_type_hash: [41u8; 32].into(),
                    backend_type: BackendType::Meta,
                    generator_path: format!("{}/meta_v0", dir.to_string_lossy()).into(),
                    validator_path: format!("{}/meta_v0", dir.to_string_lossy()).into(),
                },
                BackendConfig {
                    validator_script_type_hash: [42u8; 32].into(),
                    backend_type: BackendType::Sudt,
                    generator_path: format!("{}/sudt_v1", dir.to_string_lossy()).into(),
                    validator_path: format!("{}/sudt_v1", dir.to_string_lossy()).into(),
                },
            ],
        };
        m.register_backend_fork(config, false).unwrap();
        assert!(m.get_backends_at_height(0).is_none(), "no backends at 0");
        // sudt
        assert_eq!(
            m.get_backend(4, &[42u8; 32].into())
                .unwrap()
                .generator
                .to_vec(),
            b"sudt_v0".to_vec(),
        );
        assert_eq!(
            m.get_backend(5, &[42u8; 32].into())
                .unwrap()
                .generator
                .to_vec(),
            b"sudt_v1".to_vec(),
        );
        assert_eq!(
            m.get_backend(42, &[42u8; 32].into())
                .unwrap()
                .generator
                .to_vec(),
            b"sudt_v1".to_vec(),
        );
        // meta
        assert!(m.get_backend(1, &[41u8; 32].into()).is_none());
        assert_eq!(
            m.get_backend(5, &[41u8; 32].into())
                .unwrap()
                .generator
                .to_vec(),
            b"meta_v0".to_vec(),
        );
        assert_eq!(
            m.get_backend(42, &[41u8; 32].into())
                .unwrap()
                .generator
                .to_vec(),
            b"meta_v0".to_vec(),
        );
        // addr
        assert_eq!(
            m.get_backend(1, &[43u8; 32].into())
                .unwrap()
                .generator
                .to_vec(),
            b"addr_v0".to_vec(),
        );
        assert_eq!(
            m.get_backend(42, &[43u8; 32].into())
                .unwrap()
                .generator
                .to_vec(),
            b"addr_v0".to_vec(),
        );
    }
}
