use std::collections::HashSet;
use std::fs;
use std::io::BufReader;
use std::path::PathBuf;

use anyhow::{anyhow, bail, Result};
use gw_chain::chain::Chain;
use gw_config::Config;
use gw_store::traits::chain_store::ChainStore;
use gw_types::offchain::ExportedBlock;
use gw_types::prelude::Unpack;
use gw_utils::export_block::{
    check_block_post_state, insert_bad_block_hashes, ExportedBlockReader,
};
use indicatif::{ProgressBar, ProgressStyle};

use crate::runner::BaseInitComponents;

pub const DEFAULT_READ_BATCH: usize = 500;

pub struct ImportArgs {
    pub config: Config,
    pub source: PathBuf,
    pub read_batch: Option<usize>,
    pub to_block: Option<u64>,
    pub show_progress: bool,
}

pub struct ImportBlock {
    chain: Chain,
    source: PathBuf,
    read_batch: usize,
    to_block: Option<u64>,
    progress_bar: Option<ProgressBar>,
}

impl ImportBlock {
    pub async fn create(args: ImportArgs) -> Result<Self> {
        let base = BaseInitComponents::init(&args.config, true).await?;
        let chain = Chain::create(
            &base.rollup_config,
            &base.rollup_type_script,
            &args.config.chain,
            base.store,
            base.generator,
            None,
        )?;

        let progress_bar = if args.show_progress {
            let metadata = fs::metadata(&args.source)?;
            let bar = ProgressBar::new(metadata.len() as u64);
            bar.set_style(
                ProgressStyle::default_bar()
                    .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}")
                    .progress_chars("##-"),
            );
            Some(bar)
        } else {
            None
        };

        let import_block = ImportBlock {
            chain,
            source: args.source,
            read_batch: args.read_batch.unwrap_or(DEFAULT_READ_BATCH),
            to_block: args.to_block,
            progress_bar,
        };

        Ok(import_block)
    }

    pub fn execute(mut self) -> Result<()> {
        let store = self.chain.store();
        store.check_state()?;

        let last_valid_tip_block_hash = store.get_last_valid_tip_block_hash()?;
        let tip_block_hash = store.get_tip_block_hash()?;
        if last_valid_tip_block_hash != tip_block_hash {
            bail!("database with tip bad block");
        }

        self.read_from_mol()
    }

    pub fn read_from_mol(&mut self) -> Result<()> {
        let store = self.chain.store();
        let f = fs::File::open(&self.source)?;
        let mut block_reader = ExportedBlockReader::new(BufReader::new(f));

        // Seek new block
        let snap = store.get_snapshot();
        let db_tip_block = snap.get_tip_block()?;
        let db_tip_block_number = db_tip_block.raw().number().unpack();

        let (first_block, _size) = block_reader
            .peek_block()?
            .ok_or_else(|| anyhow!("empty file"))?;
        let first_block_number = first_block.block_number();

        if first_block_number > db_tip_block_number + 1 {
            bail!(
                "missing blocks from {} to {}",
                db_tip_block_number + 1,
                first_block_number
            )
        }

        if first_block_number <= db_tip_block_number {
            let new_block_offset = db_tip_block_number + 1 - first_block_number;
            let (n, size) = block_reader.skip_blocks(new_block_offset)?;
            if n != new_block_offset {
                bail!("no new block")
            }

            if let Some(ref progress_bar) = self.progress_bar {
                progress_bar.inc(size as u64)
            }
        }

        // Insert new blocks
        let (new_block, _size) = block_reader
            .peek_block()?
            .ok_or_else(|| anyhow!("no new block"))?;
        if new_block.parent_block_hash() != db_tip_block.hash().into() {
            bail!("diff parent block {}", db_tip_block_number);
        }

        // Read blocks in background
        let (tx, rx) = std::sync::mpsc::sync_channel(self.read_batch);
        let to_block = self.to_block;
        let read_in_background = std::thread::spawn(move || {
            for maybe_new_block in block_reader {
                match maybe_new_block.as_ref() {
                    Err(_) => return,
                    Ok((block, _size))
                        if to_block.is_some() && Some(block.block_number()) > to_block =>
                    {
                        return
                    }
                    Ok(_) => tx.send(maybe_new_block).expect("send block in background"),
                };
            }
        });

        let mut next_block_number = db_tip_block_number + 1;
        for maybe_new_block in rx.into_iter() {
            let (block, size) = maybe_new_block
                .map_err(|err| anyhow!("read block {} {}", next_block_number, err))?;
            let block_number = block.block_number();

            insert_block(&mut self.chain, block)
                .map_err(|err| anyhow!("insert block {} {}", block_number, err))?;

            if let Some(ref progress_bar) = self.progress_bar {
                progress_bar.inc(size as u64)
            }

            next_block_number += 1;
        }

        if let Some(ref progress_bar) = self.progress_bar {
            progress_bar.finish_with_message("done");
        }

        read_in_background.join().expect("join read background");

        Ok(())
    }
}

fn insert_block(chain: &mut Chain, exported: ExportedBlock) -> Result<()> {
    let tx_db = chain.store().begin_transaction();
    let block_number = exported.block_number();

    chain.process_block(
        &tx_db,
        exported.block,
        exported.committed_info,
        exported.post_global_state.clone(),
        exported.deposit_requests,
        HashSet::from_iter(exported.deposit_asset_scripts),
        exported.withdrawals,
    )?;

    // Update reverted blocks smt
    if let Some(bad_block_hashes_vec) = exported.bad_block_hashes {
        insert_bad_block_hashes(&tx_db, bad_block_hashes_vec)?;
    }

    check_block_post_state(&tx_db, block_number, &exported.post_global_state)?;

    tx_db.commit()?;

    Ok(())
}
