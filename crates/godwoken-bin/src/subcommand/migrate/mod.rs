use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use gw_config::Config;
use gw_db::migrate::{init_migration_factory, open_or_create_db};

#[cfg(feature = "smt-trie")]
mod smt_trie;

pub const COMMAND_MIGRATE: &str = "migrate";

/// Perform db migrations
#[derive(Parser)]
#[clap(name = COMMAND_MIGRATE)]
pub struct MigrateCommand {
    /// Godwoken config file path
    #[clap(long)]
    config: PathBuf,
}

impl MigrateCommand {
    pub fn run(self) -> Result<()> {
        let _guard = gw_block_producer::trace::init(None)?;

        let content = std::fs::read(&self.config)
            .with_context(|| format!("read config file from {}", self.config.to_string_lossy()))?;
        let config: Config = toml::from_slice(&content).context("parse config file")?;

        // Replace migration placeholders with real migrations, and run the migrations.
        #[allow(unused_mut)]
        let mut factory = init_migration_factory();
        #[cfg(feature = "smt-trie")]
        assert!(factory.insert(Box::new(smt_trie::SMTTrieMigration)));
        open_or_create_db(&config.store, factory).context("open and migrate database")?;

        Ok(())
    }
}
