use gw_config::ForkConfig;
use gw_jsonrpc_types::blockchain::CellDep;
use gw_types::{core::H256, packed::RollupConfig};

/// A wildly used context, contains several common-used configurations.
#[derive(Clone, Default)]
pub struct RollupContext {
    /// rollup_state_cell's type hash
    pub rollup_script_hash: H256,
    pub rollup_config: RollupConfig,
    pub fork_config: ForkConfig,
}

impl RollupContext {
    /// Returns the version of global state for `block_number`.
    pub fn global_state_version(&self, block_number: u64) -> u8 {
        self.fork_config.global_state_version(block_number)
    }

    pub fn rollup_config_cell_dep(&self) -> &CellDep {
        &self.fork_config.chain.rollup_config_cell_dep
    }
}
