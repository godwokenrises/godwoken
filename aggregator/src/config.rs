use ckb_types::packed::Script;
use gw_types::packed::L2Block;

pub struct Config {
    pub chain: ChainConfig,
    pub rpc: RPC,
    pub lumos: Lumos,
}

pub struct Signer {
    pub account_id: u32,
}

pub struct ChainConfig {
    pub signer: Option<Signer>,
    pub rollup_type_script: Script,
    pub l2_genesis: L2Block,
}

pub struct RPC {
    pub listen: String,
}

pub struct Lumos {
    pub callback: String,
    pub endpoint: String,
}
