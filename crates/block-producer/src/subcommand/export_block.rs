use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::{anyhow, bail, Result};
use gw_common::H256;
use gw_config::Config;
use gw_db::migrate::open_or_create_db;
use gw_db::read_only_db::ReadOnlyDB;
use gw_db::schema::COLUMNS;
use gw_store::readonly::StoreReadonly;
use gw_store::{traits::chain_store::ChainStore, Store};
use gw_types::packed;
use gw_types::prelude::{Entity, Unpack};
use indicatif::{ProgressBar, ProgressStyle};

pub struct ExportArgs {
    pub config: Config,
    pub output: PathBuf,
    pub from_block: Option<u64>,
    pub to_block: Option<u64>,
    pub show_progress: bool,
}

/// ExportBlock
///
/// Support export block from readonly database (don't need to exit node process)
/// NOTE: only works without reverted blocks changes between from block and to block
pub struct ExportBlock {
    snap: StoreReadonly,
    store: Option<Store>,
    output: PathBuf,
    from_block: u64,
    to_block: u64,
    progress_bar: Option<ProgressBar>,
}

impl ExportBlock {
    pub fn create(args: ExportArgs) -> Result<Self> {
        let snap = {
            let cf_names = (0..COLUMNS).map(|c| c.to_string());
            let db = ReadOnlyDB::open_cf(&args.config.store.path, cf_names)?
                .ok_or_else(|| anyhow!("no database"))?;
            StoreReadonly::new(db)
        };

        let from_block = args.from_block.unwrap_or(0);
        let to_block = match args.to_block {
            Some(to) => {
                snap.get_block_hash_by_number(to)?
                    .ok_or_else(|| anyhow!("{} block not found", to))?;
                to
            }
            None => snap.get_last_valid_tip_block()?.raw().number().unpack(),
        };
        if from_block > to_block {
            bail!("from {} is bigger than to {}", from_block, to_block);
        }

        // We need `Store` to get bad block hashes
        let store = {
            let get_reverted_block_root = |block| -> Result<H256> {
                let hash = snap
                    .get_block_hash_by_number(block)?
                    .ok_or_else(|| anyhow!("block hash {} not found", block))?;
                let state = snap
                    .get_block_post_global_state(&hash)?
                    .ok_or_else(|| anyhow!("block {} post global state not found", block))?;
                Ok(state.reverted_block_root().unpack())
            };

            let from_reverted_block_root = get_reverted_block_root(from_block)?;
            let to_reverted_block_root = get_reverted_block_root(to_block)?;

            if from_reverted_block_root == to_reverted_block_root {
                None
            } else {
                let store = Store::new(open_or_create_db(&args.config.store)?);
                Some(store)
            }
        };

        let progress_bar = if args.show_progress {
            let bar = ProgressBar::new(to_block.saturating_sub(from_block) + 1);
            bar.set_style(
                ProgressStyle::default_bar()
                    .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}")
                    .progress_chars("##-"),
            );
            Some(bar)
        } else {
            None
        };

        let output = {
            let mut output = args.output;
            let mut file_name = output
                .file_name()
                .ok_or_else(|| anyhow!("no file name in path"))?
                .to_os_string();

            file_name.push(format!("_{:x}", args.config.genesis.rollup_type_hash));
            file_name.push(format!("_{}_{}", from_block, to_block));

            output.set_file_name(file_name);
            output
        };

        let export_block = ExportBlock {
            snap,
            store,
            output,
            from_block,
            to_block,
            progress_bar,
        };

        Ok(export_block)
    }

    pub fn execute(self) -> Result<()> {
        if let Some(parent) = self.output.parent() {
            fs::create_dir_all(parent)?;
        }
        self.write_to_mol()
    }

    pub fn write_to_mol(self) -> Result<()> {
        let f = fs::OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(self.output)?;

        let mut writer = io::BufWriter::new(f);
        for block_number in self.from_block..=self.to_block {
            let exported_block = gw_utils::export_block::export_block(
                &self.snap,
                self.store.as_ref(),
                block_number,
            )?;
            let packed: packed::ExportedBlock = exported_block.into();

            writer.write_all(packed.as_slice())?;

            if let Some(ref progress_bar) = self.progress_bar {
                progress_bar.inc(1)
            }
        }

        if let Some(ref progress_bar) = self.progress_bar {
            progress_bar.finish_with_message("done");
        }

        Ok(())
    }
}
