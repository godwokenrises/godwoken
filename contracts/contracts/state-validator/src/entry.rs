// Import from `core` instead of from `std` since we are in no-std mode
use core::result::Result;

// Import heap related library from `alloc`
// https://doc.rust-lang.org/alloc/index.html
use alloc::{vec, vec::Vec};

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use ckb_std::{
    debug,
    high_level::{load_script, load_tx_hash, load_witness_args},
    ckb_types::{bytes::Bytes, prelude::*},
};

use crate::error::Error;

pub struct Context {
    number: u64,
    aggregator_id: u32,
    kv_pairs: BTreeMap<H256, H256>,
}

// TODO 1. consider contract on creation
// TODO 2. make sure we only have 1 contract cell
fn parse_l2block() -> Result<L2Block, Error> {
    let witness_args = load_witness_args(0, Source::GroupOutput)?;
    let output_type = witness_args.output_type().to_opt().ok_or_else(|| Error::Encoding)?;
    match L2BlockReader::verify(&output_type) {
        Ok(_) => Ok(L2Block::new_unchecked(output_type)),
        Err(_) => Err(Error::Encoding),
    }
}

fn parse_global_state(source: Source) -> Result<GlobalState, Error> {
    let data = load_cell_data(0, source)?;
    match GlobalStateReader::verify(&data) {
        Ok(_) => Ok(GlobalState::new_unchecked(data)),
        Err(_) => Err(Error::Encoding),
    }
}

fn verify_aggregator(aggregator_id: u32) -> Result<(), Error> {
    unimplemented!()
}

fn verify_signature(pubkey_hash: &[u8; 20], signature: &[u8; 65]) -> Result<(), Error> {
    unimplemented!()
}

fn verify_l2block(l2block: &L2Block, prev_global_state: &GlobalState) -> Result<Context, Error> {
    // check merkle proof
    let raw_block = l2block.raw();
    let prev_account_root: [u8; 32] = raw_block.prev_account().root().unpack();
    if prev_global_state.account().root().unpack() != prev_account_root {
        return Err(Error::InvalidPrevGlobalState);
    }
    let number: u64 = raw_block.number().unpack();
    if number != prev_global_state.block().count().unpack() {
        return Err(Error::InvalidPrevGlobalState);
    }
    // check merkle proof
    let proof: Bytes = l2block.proof().unpack();
    let merkle_proof = CompiledMerkleProof(proof.into());
    let kv_paris: BTreeMap<_, _> = l2block.inputs().unpack::<Vec<_>>().map(|kv| (kv.k, kv.v)).collect();
    if !merkle_proof.verify(&prev_account_root, kv_pairs.iter().collect()).map_err(|_| Error::MerkleVerify)? {
        return Err(Error::InvalidMerkleProof);
    }
    let aggregator_id: u32 = raw_block.aggregator_id().unpack();
    // verify aggregator
    verify_aggregator(aggregator_id)?;
    // check signature
    let pubkey_hash = unreachable!();
    let signature: [u8; 65] = block.signature().unpack();
    verify_signature(&pubkey_hash, &signature)?;
    let context = Context { number, aggregator_id, kv_pairs };
    Ok(context)
}

fn handle_join(context: &mut Context, raw_block: &RawBlock) -> Result<(), Error> {
    // 1. find all inputs which use the deposition-lock
    // 2. find or create accounts accoding to requests
    // 3. deposit balance to account (how? call contract, or directly alter the balance)
    unimplemented!()
}

pub fn main() -> Result<(), Error> {
    // basic verification
    let prev_global_state = parse_global_state(Source::GroupInput)?;
    let l2block = parse_l2block()?;
    let context = verify_l2block(&l2block, &prev_global_state)?;
    let raw_block = l2block.raw();

    // handle state transitions
    handle_join(&mut context, &raw_block)?;
    let script = load_script()?;
    let args: Bytes = script.args().unpack();
    debug!("script args is {:?}", args);

    let tx_hash = load_tx_hash()?;
    debug!("tx hash is {:?}", tx_hash);

    let _buf: Vec<_> = vec![0u8; 32];

    Ok(())
}

