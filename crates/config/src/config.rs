use ckb_types::H160;
use gw_jsonrpc_types::ckb_jsonrpc_types::Script;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub chain: ChainConfig,
    pub consensus: ConsensusConfig,
    pub rpc: RPC,
    pub lumos: Lumos,
    pub genesis: GenesisConfig,
    pub aggregator: Option<AggregatorConfig>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AggregatorConfig {
    pub account_id: u32,
    pub signer: SignerConfig,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignerConfig {}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConsensusConfig {
    pub aggregator_id: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GenesisConfig {
    pub initial_aggregator_pubkey_hash: H160,
    pub initial_deposition: u64,
    pub timestamp: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChainConfig {
    pub rollup_type_script: Script,
    pub genesis_block_hash: [u8; 32],
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
