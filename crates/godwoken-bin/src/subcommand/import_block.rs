use std::collections::HashSet;
use std::fs;
use std::io::BufReader;
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use gw_block_producer::runner::BaseInitComponents;
use gw_chain::chain::{Chain, RevertL1ActionContext, RevertedL1Action, SyncParam};
use gw_config::Config;
use gw_store::{traits::chain_store::ChainStore, Store};
use gw_types::{offchain::ExportedBlock, packed::NumberHash, prelude::*};
use gw_utils::export_block::{
    check_block_post_state, insert_bad_block_hashes, ExportedBlockReader,
};
use indicatif::{ProgressBar, ProgressStyle};

pub const DEFAULT_READ_BATCH: usize = 500;

pub struct ImportArgs {
    pub config: Config,
    pub source: PathBuf,
    pub read_batch: Option<usize>,
    pub to_block: Option<u64>,
    pub rewind_to_last_valid_tip: bool,
    pub show_progress: bool,
}

pub struct ImportBlock {
    chain: Chain,
    source: PathBuf,
    read_batch: usize,
    to_block: Option<u64>,
    rewind_to_last_valid_tip: bool,
    progress_bar: Option<ProgressBar>,
}

impl ImportBlock {
    // Disable warning for bin
    #[allow(dead_code)]
    pub fn new_unchecked(chain: Chain, source: PathBuf) -> Self {
        ImportBlock {
            chain,
            source,
            read_batch: DEFAULT_READ_BATCH,
            to_block: None,
            rewind_to_last_valid_tip: false,
            progress_bar: None,
        }
    }

    pub async fn create(args: ImportArgs) -> Result<Self> {
        let base = BaseInitComponents::init(&args.config, true).await?;
        let chain = Chain::create(
            base.rollup_config.clone(),
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
            rewind_to_last_valid_tip: args.rewind_to_last_valid_tip,
            progress_bar,
        };

        Ok(import_block)
    }

    // Disable warning for bin
    #[allow(dead_code)]
    pub fn store(&self) -> &Store {
        self.chain.store()
    }

    pub async fn execute(mut self) -> Result<()> {
        let store = self.chain.store();
        store.check_state()?;

        let last_valid_tip_block_hash = store.get_last_valid_tip_block_hash()?;
        let tip_block_hash = store.get_tip_block_hash()?;
        if last_valid_tip_block_hash != tip_block_hash && !self.rewind_to_last_valid_tip {
            bail!("database with tip bad block");
        }

        if self.rewind_to_last_valid_tip {
            let last_valid_tip_post_global_state = store
                .get_block_post_global_state(&last_valid_tip_block_hash)?
                .ok_or_else(|| anyhow!("last valid tip post global state not found"))?;
            let rewind_to_last_valid_tip = RevertedL1Action {
                prev_global_state: last_valid_tip_post_global_state,
                context: RevertL1ActionContext::RewindToLastValidTip,
            };

            let param = SyncParam {
                reverts: vec![rewind_to_last_valid_tip],
                updates: vec![],
            };
            self.chain.sync(param).await?;
            println!("rewind success")
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
        if new_block.parent_block_hash() != db_tip_block.hash() {
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

        let mut last_submitted_block = None;
        let mut next_block_number = db_tip_block_number + 1;
        for maybe_new_block in rx.into_iter() {
            let (block, size) = maybe_new_block
                .map_err(|err| anyhow!("read block {} {}", next_block_number, err))?;
            let block_number = block.block_number();

            insert_block(&mut self.chain, block, &mut last_submitted_block)
                .map_err(|err| anyhow!("insert block {} {}", block_number, err))?;

            if let Some(ref progress_bar) = self.progress_bar {
                progress_bar.inc(size as u64)
            }

            next_block_number += 1;
        }

        // Just set last_submitted/last_confirmed block to the last block that
        // has a submission tx, because we don't have a CKB rpc client here.
        //
        // When the node starts it will sync with L1 and correct the last
        // confirmed block.
        if let Some(last_submitted_block) = last_submitted_block {
            let tx_db = &self.chain.store().begin_transaction();
            let block_hash = tx_db
                .get_block_hash_by_number(last_submitted_block)?
                .context("get block hash")?;
            let nh = NumberHash::new_builder()
                .number(last_submitted_block.pack())
                .block_hash(block_hash.pack())
                .build();
            let nh = nh.as_reader();
            tx_db.set_last_submitted_block_number_hash(&nh)?;
            tx_db.set_last_confirmed_block_number_hash(&nh)?;
            tx_db.commit()?;
        }

        if let Some(ref progress_bar) = self.progress_bar {
            progress_bar.finish_with_message("done");
        }

        read_in_background.join().expect("join read background");

        Ok(())
    }
}

fn insert_block(
    chain: &mut Chain,
    exported: ExportedBlock,
    last_submitted_block: &mut Option<u64>,
) -> Result<()> {
    let tx_db = chain.store().begin_transaction();
    let block_number = exported.block_number();

    if let Some(_challenge_target) = chain.process_block(
        &tx_db,
        exported.block,
        exported.post_global_state.clone(),
        exported.deposit_info_vec,
        HashSet::from_iter(exported.deposit_asset_scripts),
        exported.withdrawals,
    )? {
        bail!("bad block")
    }

    // Update reverted blocks smt
    if let Some(bad_block_hashes_vec) = exported.bad_block_hashes {
        insert_bad_block_hashes(&tx_db, bad_block_hashes_vec)?;
    }

    check_block_post_state(&tx_db, block_number, &exported.post_global_state)?;

    if let Some(hash) = exported.submit_tx_hash {
        tx_db.set_block_submit_tx_hash(block_number, &hash)?;
        *last_submitted_block = Some(block_number);
    };
    chain.calculate_and_store_finalized_custodians(&tx_db, block_number)?;

    tx_db.commit()?;

    Ok(())
}
