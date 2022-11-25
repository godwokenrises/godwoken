use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use gw_config::Config;
use gw_db::migrate::{init_migration_factory, open_or_create_db};

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
        // TODO: logging.

        let config: Config = None.unwrap();

        // Replace migration placeholders with real migrations, and run the migrations.
        let mut factory = init_migration_factory();
        let replaced = factory.insert(Box::new(smt_trie::SMTTrieMigration));
        assert!(replaced);
        open_or_create_db(&config.store, factory).context("open and migrate database")?;

        Ok(())
    }
}
