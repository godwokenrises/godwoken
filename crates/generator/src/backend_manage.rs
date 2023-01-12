use anyhow::{bail, Context, Result};
use gw_config::{BackendConfig, BackendForkConfig, BackendType};
use gw_types::bytes::Bytes;
use gw_types::h256::*;
use gw_utils::checksum::content_checksum;
use std::collections::{HashMap, HashSet};

#[cfg(has_asm)]
use crate::types::vm::AotCode;

#[derive(Clone)]
pub struct Backend {
    pub generator: Bytes,
    pub validator_script_type_hash: H256,
    pub backend_type: BackendType,
    pub generator_checksum: H256,
}

impl Backend {
    pub fn build(
        backend_type: BackendType,
        validator_script_type_hash: H256,
        generator: Bytes,
        generator_checksum: H256,
    ) -> Result<Self> {
        let checksum: H256 = content_checksum(&generator);

        if generator_checksum != checksum {
            bail!(
                "Backend {:?} checksum mismatch, expected: {}, actual: {}",
                backend_type,
                hex::encode(generator_checksum),
                hex::encode(checksum)
            );
        }

        Ok(Self {
            generator,
            validator_script_type_hash,
            backend_type,
            generator_checksum,
        })
    }
}

/// SUDT Proxy config
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct SUDTProxyConfig {
    /// Should only be used in test environment
    pub permit_sudt_transfer_from_dangerous_contract: bool,
    /// Allowed sUDT proxy address list
    pub address_list: HashSet<[u8; 20]>,
}

#[derive(Clone, Default)]
pub struct BlockConsensus {
    pub sudt_proxy: SUDTProxyConfig,
    pub backends: HashMap<H256, Backend>,
}

#[derive(Default)]
pub struct BackendManage {
    backend_forks: Vec<(u64, BlockConsensus)>,
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
        // inherit block consensus
        let mut block_consensus = self
            .backend_forks
            .last()
            .map(|(_height, consensus)| consensus.clone())
            .unwrap_or_default();

        let fork_height = config.fork_height;

        // set sudt proxy
        if let Some(sudt_proxy) = config.sudt_proxy {
            block_consensus.sudt_proxy = SUDTProxyConfig {
                permit_sudt_transfer_from_dangerous_contract: sudt_proxy
                    .permit_sudt_transfer_from_dangerous_contract,
                address_list: sudt_proxy
                    .address_list
                    .into_iter()
                    .map(Into::into)
                    .collect(),
            };
        }

        if block_consensus
            .sudt_proxy
            .permit_sudt_transfer_from_dangerous_contract
        {
            log::warn!(
                "`permit_sudt_transfer_from_dangerous_contract` is set to `true` at height {}.",
                fork_height
            );
        }

        // register backends
        for config in config.backends {
            let BackendConfig {
                generator,
                generator_checksum,
                validator_script_type_hash,
                backend_type,
            } = config;
            let generator = generator
                .get()
                .with_context(|| format!("load generator from {}", generator))?
                .into_owned()
                .into();
            let backend = Backend::build(
                backend_type,
                validator_script_type_hash.into(),
                generator,
                generator_checksum.into(),
            )?;
            #[cfg(has_asm)]
            if compile {
                self.compile_backend(&backend);
            }

            log::debug!(
                "registry backend {:?}({}) at height {}",
                backend.backend_type,
                hex::encode(backend.generator_checksum),
                fork_height
            );

            block_consensus
                .backends
                .insert(backend.validator_script_type_hash, backend);
        }

        self.backend_forks
            .push((config.fork_height, block_consensus));
        Ok(())
    }

    #[cfg(has_asm)]
    fn compile_backend(&mut self, backend: &Backend) {
        self.aot_codes.insert(
            backend.generator_checksum,
            self.aot_compile(&backend.generator)
                .expect("Ahead-of-time compile"),
        );
    }

    pub fn get_block_consensus_at_height(
        &self,
        block_number: u64,
    ) -> Option<&(u64, BlockConsensus)> {
        self.backend_forks
            .iter()
            .rev()
            .find(|(height, _)| block_number >= *height)
    }

    pub fn get_mut_block_consensus_at_height(
        &mut self,
        block_number: u64,
    ) -> Option<&mut (u64, BlockConsensus)> {
        self.backend_forks
            .iter_mut()
            .rev()
            .find(|(height, _)| block_number >= *height)
    }

    pub fn get_backend(&self, block_number: u64, code_hash: &H256) -> Option<&Backend> {
        self.get_block_consensus_at_height(block_number)
            .and_then(|(_number, consensus)| consensus.backends.get(code_hash))
            .map(|backend| {
                log::debug!(
                    "get backend {:?}({}) at height {}",
                    backend.backend_type,
                    hex::encode(backend.generator_checksum),
                    block_number
                );
                backend
            })
    }

    #[cfg(has_asm)]
    fn aot_compile(&self, code_bytes: &Bytes) -> Result<AotCode, ckb_vm::Error> {
        let vm_version = crate::types::vm::VMVersion::V1;
        let mut aot_machine = ckb_vm_aot::AotCompilingMachine::load(
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
    use gw_builtin_binaries::Resource;
    use gw_config::{BackendConfig, BackendForkConfig, BackendType};
    use gw_utils::checksum::content_checksum;

    use super::BackendManage;

    #[test]
    fn test_get_block_consensus() {
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
            sudt_proxy: Some(gw_config::SUDTProxyConfig {
                permit_sudt_transfer_from_dangerous_contract: true,
                address_list: vec![[1u8; 20].into()],
            }),
            backends: vec![
                BackendConfig {
                    validator_script_type_hash: [42u8; 32].into(),
                    backend_type: BackendType::Sudt,
                    generator: Resource::file_system(
                        format!("{}/sudt_v0", dir.to_string_lossy()).into(),
                    ),
                    generator_checksum: content_checksum(b"sudt_v0").into(),
                },
                BackendConfig {
                    validator_script_type_hash: [43u8; 32].into(),
                    backend_type: BackendType::EthAddrReg,
                    generator: Resource::file_system(
                        format!("{}/addr_v0", dir.to_string_lossy()).into(),
                    ),
                    generator_checksum: content_checksum(b"addr_v0").into(),
                },
            ],
        };
        m.register_backend_fork(config, false).unwrap();
        assert!(
            m.get_block_consensus_at_height(0).is_none(),
            "no backends at 0"
        );
        assert!(m.get_backend(1, &[42u8; 32]).is_some(), "get backend at 1");
        assert!(
            m.get_backend(100, &[42u8; 32]).is_some(),
            "get backend at 100"
        );
        assert!(m.get_backend(0, &[43u8; 32]).is_none(), "get backend at 0");
        assert!(m.get_backend(1, &[43u8; 32]).is_some(), "get backend at 1");
        assert!(
            m.get_backend(100, &[43u8; 32]).is_some(),
            "get backend at 100"
        );
        // sudt proxy
        assert!(
            m.get_block_consensus_at_height(1)
                .unwrap()
                .1
                .sudt_proxy
                .permit_sudt_transfer_from_dangerous_contract
        );

        assert_eq!(
            m.get_block_consensus_at_height(1)
                .unwrap()
                .1
                .sudt_proxy
                .address_list
                .clone()
                .into_iter()
                .collect::<Vec<_>>(),
            vec![[1u8; 20]]
        );
        let config = BackendForkConfig {
            fork_height: 5,
            sudt_proxy: Some(gw_config::SUDTProxyConfig {
                permit_sudt_transfer_from_dangerous_contract: false,
                address_list: vec![[42u8; 20].into()],
            }),
            backends: vec![
                BackendConfig {
                    validator_script_type_hash: [41u8; 32].into(),
                    backend_type: BackendType::Meta,
                    generator: Resource::file_system(
                        format!("{}/meta_v0", dir.to_string_lossy()).into(),
                    ),
                    generator_checksum: content_checksum(b"meta_v0").into(),
                },
                BackendConfig {
                    validator_script_type_hash: [42u8; 32].into(),
                    backend_type: BackendType::Sudt,
                    generator: Resource::file_system(
                        format!("{}/sudt_v1", dir.to_string_lossy()).into(),
                    ),
                    generator_checksum: content_checksum(b"sudt_v1").into(),
                },
            ],
        };
        m.register_backend_fork(config, false).unwrap();
        assert!(
            m.get_block_consensus_at_height(0).is_none(),
            "no backends at 0"
        );
        // sudt
        assert_eq!(
            m.get_backend(4, &[42u8; 32]).unwrap().generator.to_vec(),
            b"sudt_v0".to_vec(),
        );
        assert_eq!(
            m.get_backend(5, &[42u8; 32]).unwrap().generator.to_vec(),
            b"sudt_v1".to_vec(),
        );
        assert_eq!(
            m.get_backend(42, &[42u8; 32]).unwrap().generator.to_vec(),
            b"sudt_v1".to_vec(),
        );
        // meta
        assert!(m.get_backend(1, &[41u8; 32]).is_none());
        assert_eq!(
            m.get_backend(5, &[41u8; 32]).unwrap().generator.to_vec(),
            b"meta_v0".to_vec(),
        );
        assert_eq!(
            m.get_backend(42, &[41u8; 32]).unwrap().generator.to_vec(),
            b"meta_v0".to_vec(),
        );
        // addr
        assert_eq!(
            m.get_backend(1, &[43u8; 32]).unwrap().generator.to_vec(),
            b"addr_v0".to_vec(),
        );
        assert_eq!(
            m.get_backend(42, &[43u8; 32]).unwrap().generator.to_vec(),
            b"addr_v0".to_vec(),
        );
        // sudt proxy
        assert!(
            !m.get_block_consensus_at_height(42)
                .unwrap()
                .1
                .sudt_proxy
                .permit_sudt_transfer_from_dangerous_contract
        );

        assert_eq!(
            m.get_block_consensus_at_height(42)
                .unwrap()
                .1
                .sudt_proxy
                .address_list
                .clone()
                .into_iter()
                .collect::<Vec<_>>(),
            vec![[42u8; 20]]
        );

        // test sudt inherited
        let config = BackendForkConfig {
            fork_height: 50,
            sudt_proxy: None,
            backends: vec![],
        };
        m.register_backend_fork(config, false).unwrap();
        // sudt proxy
        assert!(
            !m.get_block_consensus_at_height(55)
                .unwrap()
                .1
                .sudt_proxy
                .permit_sudt_transfer_from_dangerous_contract
        );

        assert_eq!(
            m.get_block_consensus_at_height(55)
                .unwrap()
                .1
                .sudt_proxy
                .address_list
                .clone()
                .into_iter()
                .collect::<Vec<_>>(),
            vec![[42u8; 20]]
        );
    }
}
