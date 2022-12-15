use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{ArgGroup, Parser};
use gw_config::{Config, StoreConfig};
use gw_store::migrate::{init_migration_factory, open_or_create_db};
use gw_telemetry::trace;

#[cfg(feature = "smt-trie")]
mod smt_trie;

pub const COMMAND_MIGRATE: &str = "migrate";

/// Perform db migrations
#[derive(Parser)]
#[clap(name = COMMAND_MIGRATE)]
// One (and only one) of config or db must be present.
#[clap(group(ArgGroup::new("db-or-config").required(true)))]
pub struct MigrateCommand {
    /// Godwoken config file path
    #[clap(short, long, group = "db-or-config")]
    config: Option<PathBuf>,
    /// Db path
    #[clap(long, group = "db-or-config")]
    db: Option<PathBuf>,
}

impl MigrateCommand {
    pub fn run(self) -> Result<()> {
        let _guard = trace::init()?;

        let store_config = if let Some(ref config_path) = self.config {
            let content = std::fs::read(config_path).with_context(|| {
                format!("read config file from {}", config_path.to_string_lossy())
            })?;
            let config: Config = toml::from_slice(&content).context("parse config file")?;
            config.store
        } else {
            StoreConfig {
                path: self.db.unwrap(),
                ..Default::default()
            }
        };

        // Replace migration placeholders with real migrations, and run the migrations.
        #[allow(unused_mut)]
        let mut factory = init_migration_factory();
        #[cfg(feature = "smt-trie")]
        assert!(factory.insert(Box::new(smt_trie::SMTTrieMigration)));
        open_or_create_db(&store_config, factory).context("open and migrate database")?;

        Ok(())
    }
}
