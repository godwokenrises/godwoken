use ckb_jsonrpc_types::Script;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub chain: ChainConfig,
    pub consensus: ConsensusConfig,
    pub rpc: RPC,
    pub lumos: Lumos,
    pub genesis: GenesisConfig,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Signer {
    pub account_id: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConsensusConfig {
    pub aggregator_id: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GenesisConfig {
    pub initial_aggregator_pubkey: [u8; 20],
    pub initial_deposition: u64,
    pub timestamp: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChainConfig {
    pub signer: Option<Signer>,
    pub rollup_type_script: Script,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RPC {
    pub listen: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lumos {
    pub callback: String,
    pub endpoint: String,
}
