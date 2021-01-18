use crate::parameter::GenesisConfig;
use ckb_jsonrpc_types::JsonBytes;
use gw_generator::genesis;
use gw_types::{packed, prelude::*};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct GenesisWithGlobalState {
    pub genesis: JsonBytes,
    pub global_state: JsonBytes,
}

impl From<GenesisWithGlobalState> for genesis::GenesisWithGlobalState {
    fn from(genesis: GenesisWithGlobalState) -> Self {
        genesis::GenesisWithGlobalState {
            genesis: packed::L2Block::from_slice(genesis.genesis.into_bytes().as_ref())
                .expect("Build packed::L2Block from slice"),
            global_state: packed::GlobalState::from_slice(
                genesis.global_state.into_bytes().as_ref(),
            )
            .expect("Build packed::GlobalState from slice"),
        }
    }
}

impl From<genesis::GenesisWithGlobalState> for GenesisWithGlobalState {
    fn from(genesis: genesis::GenesisWithGlobalState) -> Self {
        GenesisWithGlobalState {
            genesis: JsonBytes::from_bytes(genesis.genesis.as_bytes()),
            global_state: JsonBytes::from_bytes(genesis.global_state.as_bytes()),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct GenesisSetup {
    pub genesis_config: GenesisConfig,
    pub header_info: JsonBytes,
}
