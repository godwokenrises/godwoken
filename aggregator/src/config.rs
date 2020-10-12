use ckb_types::packed::Script;
use gw_types::packed::RawL2Block;

pub struct Config {
    pub rollup: Rollup,
    pub rpc: RPC,
    pub lumos: Lumos,
}

pub struct Rollup {
    pub rollup_type_script: Script,
    pub l2_genesis: RawL2Block,
}

pub struct RPC {
    pub listen: String,
}

pub struct Lumos {
    pub callback: String,
    pub endpoint: String,
}
