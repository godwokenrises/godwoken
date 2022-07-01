use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::{anyhow, bail, Result};
use gw_config::Config;
use gw_store::{traits::chain_store::ChainStore, Store};
use gw_types::packed;
use gw_types::prelude::{Entity, Unpack};
use indicatif::{ProgressBar, ProgressStyle};

use crate::runner::BaseInitComponents;

pub struct ExportArgs {
    pub config: Config,
    pub output: PathBuf,
    pub from_block: Option<u64>,
    pub to_block: Option<u64>,
    pub show_progress: bool,
}

pub struct ExportBlock {
    store: Store,
    output: PathBuf,
    from_block: u64,
    to_block: u64,
    progress_bar: Option<ProgressBar>,
}

impl ExportBlock {
    pub async fn create(args: ExportArgs) -> Result<Self> {
        let base = BaseInitComponents::init(&args.config, true).await?;
        let store = base.store;

        let snap = store.get_snapshot();
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

        let export_block = ExportBlock {
            store,
            output: args.output,
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
        let mut buf = Vec::new();
        for block_number in self.from_block..=self.to_block {
            let exported_block = gw_utils::export_block::export_block(&self.store, block_number)?;
            let packed: packed::ExportedBlock = exported_block.into();

            buf.resize(packed.as_slice().len() * 2, 0);
            faster_hex::hex_encode(packed.as_slice(), &mut buf)?;

            writer.write_all(&buf)?;
            writer.write_all(b"\n")?;

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
