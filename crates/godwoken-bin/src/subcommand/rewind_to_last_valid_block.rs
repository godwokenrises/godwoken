use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use gw_block_producer::runner::BaseInitComponents;
use gw_chain::chain::{Chain, RevertL1ActionContext, RevertedL1Action, SyncParam};
use gw_store::traits::chain_store::ChainStore;

pub const COMMAND_REWIND_TO_LAST_VALID_BLOCK: &str = "rewind-to-last-valid-block";

/// Rewind to last valid block
#[derive(Parser)]
#[clap(name = COMMAND_REWIND_TO_LAST_VALID_BLOCK)]
pub struct RewindToLastValidBlockCommand {
    /// The config file path
    #[clap(short, long, default_value = "./config.toml")]
    config_path: PathBuf,
}

impl RewindToLastValidBlockCommand {
    pub async fn run(self) -> Result<()> {
        let content = std::fs::read(&self.config_path).with_context(|| {
            format!(
                "read config file from {}",
                self.config_path.to_string_lossy()
            )
        })?;
        let config = toml::from_slice(&content).context("parse config file")?;
        let base = BaseInitComponents::init(&config, true).await?;
        let store = base.store.clone();
        let mut chain = Chain::create(
            &base.rollup_config,
            &base.rollup_type_script,
            &config.chain,
            base.store,
            base.generator,
            None,
        )?;
        let last_valid_tip_block_hash = store.get_last_valid_tip_block_hash()?;
        let last_valid_tip_post_global_state = store
            .get_block_post_global_state(&last_valid_tip_block_hash)?
            .context("last valid tip post global state not found")?;
        let rewind_to_last_valid_tip = RevertedL1Action {
            prev_global_state: last_valid_tip_post_global_state,
            context: RevertL1ActionContext::RewindToLastValidTip,
        };

        let param = SyncParam {
            reverts: vec![rewind_to_last_valid_tip],
            updates: vec![],
        };
        chain.sync(param).await?;

        Ok(())
    }
}
