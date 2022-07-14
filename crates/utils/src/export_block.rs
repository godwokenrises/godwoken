use std::io::{ErrorKind, Read, Seek, SeekFrom};

use anyhow::{anyhow, bail, Result};
use gw_common::{h256_ext::H256Ext, H256};
use gw_store::{
    readonly::StoreReadonly, state::state_db::StateContext, traits::chain_store::ChainStore,
    transaction::StoreTransaction,
};
use gw_types::{
    bytes::Bytes,
    offchain::ExportedBlock,
    packed::{self, DepositRequest, GlobalState},
    prelude::{Builder, Entity, Pack, Reader, Unpack},
};

// pub fn export_block(store: &Store, block_number: u64) -> Result<ExportedBlock> {
pub fn export_block(snap: &StoreReadonly, block_number: u64) -> Result<ExportedBlock> {
    let block_hash = snap
        .get_block_hash_by_number(block_number)?
        .ok_or_else(|| anyhow!("block {} not found", block_number))?;

    let block = snap
        .get_block(&block_hash)?
        .ok_or_else(|| anyhow!("block {} not found", block_number))?;

    let post_global_state = snap
        .get_block_post_global_state(&block_hash)?
        .ok_or_else(|| anyhow!("block {} post global state not found", block_number))?;

    // TODO.
    let deposit_requests: Vec<DepositRequest> = None.unwrap();

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

    let bad_block_hashes = get_bad_block_hashes(snap, block_number)?;

    let exported_block = ExportedBlock {
        block,
        post_global_state,
        deposit_requests,
        deposit_asset_scripts,
        withdrawals,
        bad_block_hashes,
    };

    Ok(exported_block)
}

pub fn read_block_size(reader: &mut impl Read) -> Result<Option<u32>> {
    let mut full_size_buf = [0u8; 4];

    let mut n = 0;
    let mut buf: &mut [u8] = &mut full_size_buf;
    while n != 4 {
        match reader.read(buf) {
            Ok(0) if 0 == n => return Ok(None),
            Ok(0) => bail!("block corrupted, full size header"),
            Ok(read) => {
                n += read;
                if 4 == n {
                    break;
                }

                buf = &mut buf[read..];
            }
            Err(ref e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => bail!(e),
        }
    }

    let full_size = u32::from_le_bytes(full_size_buf);
    Ok(Some(full_size))
}

pub fn read_block(reader: &mut impl Read) -> Result<Option<(ExportedBlock, usize)>> {
    let (full_size_bytes, full_size) = match read_block_size(reader)? {
        Some(size) => (size.to_le_bytes(), size as usize),
        None => return Ok(None),
    };
    if full_size <= 4 {
        bail!("block corrupted, full size {}", full_size);
    }

    let mut buf = vec![0; full_size];
    buf[..4].copy_from_slice(&full_size_bytes);
    reader.read_exact(&mut buf[4..full_size])?;

    packed::ExportedBlockReader::verify(&buf, false)?;
    let packed = packed::ExportedBlock::new_unchecked(Bytes::from(buf));
    Ok(Some((packed.into(), full_size)))
}

pub struct ExportedBlockReader<Reader: Read + Seek> {
    inner: Reader,
}

impl<Reader: Read + Seek> ExportedBlockReader<Reader> {
    pub fn new(reader: Reader) -> Self {
        ExportedBlockReader { inner: reader }
    }

    pub fn peek_block(&mut self) -> Result<Option<(ExportedBlock, usize)>> {
        let pos = self.inner.stream_position()?;
        let block = read_block(&mut self.inner)?;
        self.inner.seek(SeekFrom::Start(pos))?;
        Ok(block)
    }

    pub fn skip_blocks(&mut self, blocks: u64) -> Result<(u64, u64)> {
        let mut count = 0;
        let mut size = 0;

        let from_block = match self.peek_block()? {
            Some((block, _size)) => block.block_number(),
            None => return Ok((count, size)),
        };

        while count < blocks {
            let pos = self.inner.stream_position()?;

            let full_size = match read_block_size(&mut self.inner)? {
                Some(size) => size,
                None => return Ok((count, size)),
            };
            let offset = full_size.saturating_sub(4);

            let new_pos = self.inner.seek(SeekFrom::Current(offset as i64))?;
            if new_pos.saturating_sub(pos) != full_size as u64 {
                bail!("block {} corrupted", from_block + count);
            }

            count += 1;
            size += full_size as u64;
        }

        Ok((count, size))
    }
}

impl<Reader: Read + Seek> Iterator for ExportedBlockReader<Reader> {
    type Item = Result<(ExportedBlock, usize)>;

    fn next(&mut self) -> Option<Self::Item> {
        read_block(&mut self.inner).transpose()
    }
}

pub fn insert_bad_block_hashes(
    tx_db: &StoreTransaction,
    bad_block_hashes_vec: Vec<Vec<H256>>,
) -> Result<()> {
    let mut reverted_block_smt = tx_db.reverted_block_smt()?;
    for bad_block_hashes in bad_block_hashes_vec {
        let prev_smt_root = *reverted_block_smt.root();
        for block_hash in bad_block_hashes.iter() {
            reverted_block_smt.update(*block_hash, H256::one())?;
        }
        tx_db.set_reverted_block_hashes(
            reverted_block_smt.root(),
            prev_smt_root,
            bad_block_hashes,
        )?;
    }
    tx_db.set_reverted_block_smt_root(*reverted_block_smt.root())?;

    Ok(())
}

pub fn check_block_post_state(
    tx_db: &StoreTransaction,
    block_number: u64,
    post_global_state: &GlobalState,
) -> Result<()> {
    // Check account smt
    let expected_account_smt = post_global_state.account();
    let replicate_account_smt = tx_db.state_tree(StateContext::ReadOnly)?.get_merkle_state();
    if replicate_account_smt.as_slice() != expected_account_smt.as_slice() {
        bail!("replicate block {} account smt diff", block_number);
    }

    // Check block smt
    let expected_block_smt = post_global_state.block();
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
    let expected_reverted_block_root: H256 = post_global_state.reverted_block_root().unpack();
    let replicate_reverted_block_root = tx_db.get_reverted_block_smt_root()?;
    if replicate_reverted_block_root != expected_reverted_block_root {
        bail!("replicate block {} reverted block root diff", block_number);
    }

    // Check tip block hash
    let expected_tip_block_hash: H256 = post_global_state.tip_block_hash().unpack();
    let replicate_tip_block_hash = tx_db.get_last_valid_tip_block_hash()?;
    if replicate_tip_block_hash != expected_tip_block_hash {
        bail!("replicate block {} tip block hash diff", block_number);
    }

    Ok(())
}

fn get_bad_block_hashes(snap: &StoreReadonly, block_number: u64) -> Result<Option<Vec<Vec<H256>>>> {
    let parent_reverted_block_root = {
        let parent_block_number = block_number.saturating_sub(1);
        get_block_reverted_block_root(snap, parent_block_number)?
    };
    let reverted_block_root = get_block_reverted_block_root(snap, block_number)?;
    if reverted_block_root == parent_reverted_block_root {
        return Ok(None);
    }

    let mut bad_block_hashes = Vec::with_capacity(2);
    let reverted_root_iter = snap.iter_reverted_block_smt_root(reverted_block_root);
    for (reverted_block_root, reverted_block_hashes) in reverted_root_iter {
        if reverted_block_root == parent_reverted_block_root {
            break;
        }

        bad_block_hashes.push(reverted_block_hashes);
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
