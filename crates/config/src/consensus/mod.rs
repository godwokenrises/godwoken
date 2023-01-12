use serde::{Deserialize, Serialize};

use crate::ForkConfig;

mod builtins;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum BuiltinConsensus {
    Mainnet,
    Testnet,
}

/// Represents a consensus config
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum Consensus {
    /// Builtin consensus config
    Builtin {
        /// The identifier of the builtin consensus config.
        builtin: BuiltinConsensus,
    },
    /// Customized config
    Config {
        /// The config
        config: Box<ForkConfig>,
    },
}

impl Consensus {
    pub fn get_config(&self) -> &ForkConfig {
        match self {
            Consensus::Builtin { builtin } => match builtin {
                BuiltinConsensus::Mainnet => builtins::mainnet(),
                BuiltinConsensus::Testnet => builtins::testnet(),
            },
            Consensus::Config { config } => config,
        }
    }
}

impl Default for Consensus {
    fn default() -> Self {
        Consensus::Config {
            config: Default::default(),
        }
    }
}
