use anyhow::{anyhow, bail, Result};
use gw_common::H256;
use gw_store::{traits::chain_store::ChainStore, Store};
use gw_types::{
    offchain::ExportedBlock,
    prelude::{Pack, Unpack},
};

// pub fn export_block(store: &Store, block_number: u64) -> Result<ExportedBlock> {
pub fn export_block(
    snap: &impl ChainStore,
    store: Option<&Store>,
    block_number: u64,
) -> Result<ExportedBlock> {
    let block_hash = snap
        .get_block_hash_by_number(block_number)?
        .ok_or_else(|| anyhow!("block {} not found", block_number))?;

    let block = snap
        .get_block(&block_hash)?
        .ok_or_else(|| anyhow!("block {} not found", block_number))?;

    let committed_info = snap
        .get_l2block_committed_info(&block_hash)?
        .ok_or_else(|| anyhow!("block {} committed info not found", block_number))?;

    let post_global_state = snap
        .get_block_post_global_state(&block_hash)?
        .ok_or_else(|| anyhow!("block {} post global state not found", block_number))?;

    let deposit_requests = snap
        .get_block_deposit_requests(&block_hash)?
        .unwrap_or_default();

    let deposit_asset_scripts = {
        let asset_hashes = deposit_requests.iter().filter_map(|r| {
            let h: H256 = r.sudt_script_hash().unpack();
            if h.is_zero() {
                None
            } else {
                Some(h)
            }
        });
        let asset_scripts = asset_hashes.map(|h| {
            snap.get_asset_script(&h)?.ok_or_else(|| {
                anyhow!("block {} asset script {} not found", block_number, h.pack())
            })
        });
        asset_scripts.collect::<Result<Vec<_>>>()?
    };

    let withdrawals = {
        let reqs = block.as_reader().withdrawals();
        let extra_reqs = reqs.iter().map(|w| {
            let h = w.hash().into();
            snap.get_withdrawal(&h)?
                .ok_or_else(|| anyhow!("block {} withdrawal {} not found", block_number, h.pack()))
        });
        extra_reqs.collect::<Result<Vec<_>>>()?
    };

    let reverted_block_root: H256 = post_global_state.reverted_block_root().unpack();
    let bad_block_hashes = if reverted_block_root.is_zero() {
        None
    } else {
        let store = match store {
            Some(s) => s,
            None => bail!(
                "export block {} with non-zero reverted block root from readonly db",
                block_number
            ),
        };
        get_bad_block_hashes(store, block_number)?
    };

    let exported_block = ExportedBlock {
        block,
        committed_info,
        post_global_state,
        deposit_requests,
        deposit_asset_scripts,
        withdrawals,
        bad_block_hashes,
    };

    Ok(exported_block)
}

fn get_bad_block_hashes(store: &Store, block_number: u64) -> Result<Option<Vec<Vec<H256>>>> {
    let tx_db = store.begin_transaction();

    let parent_reverted_block_root = {
        let parent_block_number = block_number.saturating_sub(1);
        get_block_reverted_block_root(&tx_db, parent_block_number)?
    };
    let mut reverted_block_root = get_block_reverted_block_root(&tx_db, block_number)?;
    if reverted_block_root == parent_reverted_block_root {
        return Ok(None);
    }

    let mut bad_block_hashes = Vec::with_capacity(2);
    while reverted_block_root != parent_reverted_block_root {
        let reverted_block_hashes = tx_db
            .get_reverted_block_hashes_by_root(&reverted_block_root)?
            .ok_or_else(|| anyhow!("block {} reverted block hashes not found", block_number))?;
        bad_block_hashes.push(reverted_block_hashes.clone());

        tx_db.rewind_reverted_block_smt(reverted_block_hashes)?;
        reverted_block_root = tx_db.get_reverted_block_smt_root()?;
    }

    bad_block_hashes.reverse();
    Ok(Some(bad_block_hashes))
}

fn get_block_reverted_block_root(snap: &impl ChainStore, block_number: u64) -> Result<H256> {
    let block_hash = snap
        .get_block_hash_by_number(block_number)?
        .ok_or_else(|| anyhow!("block {} not found", block_number))?;

    let post_global_state = snap
        .get_block_post_global_state(&block_hash)?
        .ok_or_else(|| anyhow!("block {} post global state not found", block_number))?;

    Ok(post_global_state.reverted_block_root().unpack())
}
