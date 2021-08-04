use sparse_merkle_tree::H256;

use crate::packed::RollupConfig;

#[derive(Clone)]
pub struct RollupContext {
    pub rollup_script_hash: H256,
    pub rollup_config: RollupConfig,
}
