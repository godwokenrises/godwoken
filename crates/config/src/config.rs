use gw_types::packed::Script;

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub chain: ChainConfig,
    pub consensus: ConsensusConfig,
    pub rpc: RPC,
    pub genesis: GenesisConfig,
    pub aggregator: Option<AggregatorConfig>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AggregatorConfig {
    pub account_id: u32,
    pub signer: SignerConfig,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SignerConfig {}

#[derive(Clone, Debug, PartialEq)]
pub struct ConsensusConfig {
    pub aggregator_id: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GenesisConfig {
    pub timestamp: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChainConfig {
    pub rollup_type_script: Script,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RPC {
    pub listen: String,
}
