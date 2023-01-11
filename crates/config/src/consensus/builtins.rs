use crate::ForkConfig;
use lazy_static::lazy_static;

pub fn testnet() -> ForkConfig {
    lazy_static! {
        pub static ref CONFIG: ForkConfig = {
            let content = include_str!("builtins/testnet.toml");
            toml::from_str(content).expect("builtin testnet config")
        };
    }
    CONFIG.clone()
}

pub fn mainnet() -> ForkConfig {
    lazy_static! {
        pub static ref CONFIG: ForkConfig = {
            let content = include_str!("builtins/mainnet.toml");
            toml::from_str(content).expect("builtin mainnet config")
        };
    }
    CONFIG.clone()
}

#[cfg(not(feature = "no-builtin"))]
#[cfg(test)]
mod tests {
    use ckb_fixed_hash::H256;
    use gw_utils::checksum::content_checksum;

    #[test]
    fn test_builtin_testnet_config() {
        let config = super::testnet();
        for f in config.backend_forks {
            for b in f.backends {
                let checksum: H256 = content_checksum(
                    b.generator
                        .get()
                        .unwrap_or_else(|_| panic!("can't find: {}", b.generator))
                        .as_ref(),
                )
                .unwrap()
                .into();
                if checksum != b.generator_checksum {
                    panic!(
                        "actual {}, expected {}, path {}",
                        checksum, b.generator_checksum, b.generator
                    );
                }
            }
        }
    }

    #[test]
    fn test_builtin_mainnet_config() {
        let config = super::mainnet();
        for f in config.backend_forks {
            for b in f.backends {
                let checksum: H256 = content_checksum(
                    b.generator
                        .get()
                        .unwrap_or_else(|_| panic!("can't find: {}", b.generator))
                        .as_ref(),
                )
                .unwrap()
                .into();
                if checksum != b.generator_checksum {
                    panic!(
                        "actual {}, expected {}, path {}",
                        checksum, b.generator_checksum, b.generator
                    );
                }
            }
        }
    }
}
