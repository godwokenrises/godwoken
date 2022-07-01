use std::collections::HashSet;
use std::fs;
use std::io::{self, BufRead};
use std::path::PathBuf;

use anyhow::{anyhow, bail, Result};
use gw_chain::chain::Chain;
use gw_common::{h256_ext::H256Ext, H256};
use gw_config::Config;
use gw_store::{state::state_db::StateContext, traits::chain_store::ChainStore, Store};
use gw_types::prelude::{Builder, Entity, Pack, Unpack};
use gw_types::{offchain::ExportedBlock, packed};
use indicatif::{ProgressBar, ProgressStyle};

use crate::runner::BaseInitComponents;

pub struct ImportArgs {
    pub config: Config,
    pub source: PathBuf,
    pub show_progress: bool,
}

pub struct ImportBlock {
    chain: Chain,
    source: PathBuf,
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
            progress_bar,
        };

        Ok(import_block)
    }

    pub fn execute(mut self) -> Result<()> {
        self.read_from_mol()
    }

    pub fn read_from_mol(&mut self) -> Result<()> {
        let store = self.chain.store();
        let snap = store.get_snapshot();

        let f = fs::File::open(&self.source)?;
        let lines = io::BufReader::new(f).lines();
        let mut buf = Vec::new();
        let mut blocks_with_len = lines
            .map(|maybe_line| {
                let line = maybe_line?;
                let len = line.as_bytes().len();

                buf.resize(len.saturating_div(2), 0);
                faster_hex::hex_decode(line.as_bytes(), &mut buf)?;

                let packed = packed::ExportedBlock::from_slice(&buf)?;
                Result::<_, anyhow::Error>::Ok((ExportedBlock::from(packed), len))
            })
            .peekable();

        // Check exists parent blocks in db
        let (first_block, _len) = {
            let maybe_block = blocks_with_len
                .peek()
                .ok_or_else(|| anyhow!("empty file"))?;
            maybe_block.as_ref().map_err(|err| anyhow!("{}", err))?
        };
        if 0 != first_block.block_number() {
            check_parent_blocks(&snap, first_block)?;
        }

        // Seek to first new block
        let mut new_block = None;
        for maybe_block in blocks_with_len.by_ref() {
            let (block, len) = maybe_block?;
            match snap.get_block_hash_by_number(block.block_number())? {
                Some(block_hash_in_db) if block.block_hash() == block_hash_in_db => {
                    check_block(store, &block)?;
                }
                Some(_) => bail!("diff chain block {}", block.block_number()),
                None => {
                    new_block = Some((block, len));
                    break;
                }
            }

            if let Some(ref progress_bar) = self.progress_bar {
                progress_bar.inc(len as u64)
            }
        }

        // Insert new blocks
        while let Some((block, len)) = new_block.take() {
            insert_block(&mut self.chain, block)?;

            if let Some(ref progress_bar) = self.progress_bar {
                progress_bar.inc(len as u64)
            }

            new_block = blocks_with_len.next().transpose()?;
        }

        if let Some(ref progress_bar) = self.progress_bar {
            progress_bar.finish_with_message("done");
        }

        Ok(())
    }
}

fn check_parent_blocks(snap: &impl ChainStore, block: &ExportedBlock) -> Result<()> {
    let mut parent_block_hash = block.parent_block_hash();
    let mut parent_block_number = block.block_number().saturating_sub(1);

    loop {
        let parent_block = snap
            .get_block(&parent_block_hash)?
            .ok_or_else(|| anyhow!("parent block {} not found", parent_block_number))?;

        if parent_block.raw().number().unpack() != parent_block_number {
            bail!("diff parent block number {}", parent_block_number);
        }
        if 0 == parent_block_number {
            break;
        }

        parent_block_hash = parent_block.raw().parent_block_hash().unpack();
        parent_block_number = parent_block_number.saturating_sub(1);
    }

    Ok(())
}

fn check_block(store: &Store, block: &ExportedBlock) -> Result<()> {
    let db_block = gw_utils::export_block::export_block(store, block.block_number())?;
    if &db_block != block {
        bail!("diff block {}", block.block_number());
    }

    Ok(())
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
        let mut reverted_block_smt = tx_db.reverted_block_smt()?;
        for bad_block_hashes in bad_block_hashes_vec {
            for block_hash in bad_block_hashes.iter() {
                reverted_block_smt.update(*block_hash, H256::one())?;
            }
            tx_db.set_reverted_block_hashes(reverted_block_smt.root(), bad_block_hashes)?;
        }
        tx_db.set_reverted_block_smt_root(*reverted_block_smt.root())?;
    }

    // Check account smt
    let expected_account_smt = exported.post_global_state.account();
    let replicate_account_smt = tx_db.state_tree(StateContext::ReadOnly)?.get_merkle_state();
    if replicate_account_smt.as_slice() != expected_account_smt.as_slice() {
        bail!("replicate block {} account smt diff", block_number);
    }

    // Check block smt
    let expected_block_smt = exported.post_global_state.block();
    let replicate_block_smt = {
        let root = tx_db.get_block_smt_root()?;
        packed::BlockMerkleState::new_builder()
            .merkle_root(root.pack())
            .count((block_number + 1).pack())
            .build()
    };
    if replicate_block_smt.as_slice() != expected_block_smt.as_slice() {
        bail!("replicate block {} block smt diff", block_number);
    }

    // Check reverted block root
    let expected_reverted_block_root: H256 =
        exported.post_global_state.reverted_block_root().unpack();
    let replicate_reverted_block_root = tx_db.get_reverted_block_smt_root()?;
    if replicate_reverted_block_root != expected_reverted_block_root {
        bail!("replicate block {} reverted block root diff", block_number);
    }

    // Check tip block hash
    let expected_tip_block_hash: H256 = exported.post_global_state.tip_block_hash().unpack();
    let replicate_tip_block_hash = tx_db.get_last_valid_tip_block_hash()?;
    if replicate_tip_block_hash != expected_tip_block_hash {
        bail!("replicate block {} tip block hash diff", block_number);
    }

    tx_db.commit()?;

    Ok(())
}
