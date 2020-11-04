use ckb_types::packed::Script;
use gw_types::packed::L2Block;

pub struct Config {
    pub chain: ChainConfig,
    pub consensus: ConsensusConfig,
    pub rpc: RPC,
    pub lumos: Lumos,
    pub genesis: GenesisConfig,
}

pub struct Signer {
    pub account_id: u32,
}

pub struct ConsensusConfig {
    pub aggregator_id: u32,
}

pub struct GenesisConfig {
    pub initial_aggregator_pubkey: [u8; 20],
    pub initial_deposition: u64,
    pub timestamp: u64,
}

pub struct ChainConfig {
    pub signer: Option<Signer>,
    pub rollup_type_script: Script,
}

pub struct RPC {
    pub listen: String,
}

pub struct Lumos {
    pub callback: String,
    pub endpoint: String,
}
