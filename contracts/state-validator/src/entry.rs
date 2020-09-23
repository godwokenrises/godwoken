// Import from `core` instead of from `std` since we are in no-std mode
use core::result::Result;

// Import heap related library from `alloc`
// https://doc.rust-lang.org/alloc/index.html
use alloc::{collections::BTreeMap, vec, vec::Vec};

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use ckb_std::{
    ckb_constants::Source,
    ckb_types::{bytes::Bytes, prelude::*},
    debug,
    dynamic_loading::CKBDLContext,
    high_level::{load_cell_data, load_script, load_script_hash, load_tx_hash, load_witness_args},
};

use godwoken_types::{
    packed::{GlobalState, GlobalStateReader, L2Block, L2BlockReader},
    prelude::{Reader as GodwokenTypesReader, Unpack as GodwokenTypesUnpack},
};

use ckb_lib_secp256k1::LibSecp256k1;
use sparse_merkle_tree::{CompiledMerkleProof};

use crate::actions;
use crate::blake2b::{new_blake2b, Blake2bHasher};
use crate::context::Context;
use crate::error::Error;

// TODO 1. consider contract on creation
// TODO 2. make sure we only have 1 contract cell
fn parse_l2block() -> Result<L2Block, Error> {
    let witness_args = load_witness_args(0, Source::GroupOutput)?;
    let output_type: Bytes = witness_args
        .output_type()
        .to_opt()
        .ok_or_else(|| Error::Encoding)?
        .unpack();
    match L2BlockReader::verify(&output_type, false) {
        Ok(_) => Ok(L2Block::new_unchecked(output_type)),
        Err(_) => Err(Error::Encoding),
    }
}

fn parse_global_state(source: Source) -> Result<GlobalState, Error> {
    let data = load_cell_data(0, source)?;
    match GlobalStateReader::verify(&data, false) {
        Ok(_) => Ok(GlobalState::new_unchecked(data.into())),
        Err(_) => Err(Error::Encoding),
    }
}

fn verify_aggregator(aggregator_id: u32) -> Result<(), Error> {
    unimplemented!()
}

fn verify_block_signature(
    context: &Context,
    lib_secp256k1: &LibSecp256k1,
    l2block: &L2Block,
) -> Result<(), Error> {
    let pubkey_hash = context
        .get_pubkey_hash(context.aggregator_id)
        .ok_or_else(|| Error::KVMissing)?;
    let raw_block = l2block.raw();
    let message = {
        let mut message = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(raw_block.as_slice());
        hasher.finalize(&mut message);
        message
    };
    let signature: [u8; 65] = l2block.signature().unpack();
    let mut actual_pubkey_hash = [0u8; 20];
    let prefilled_data = lib_secp256k1
        .load_prefilled_data()
        .map_err(|err| Error::Secp256k1)?;
    let pubkey = lib_secp256k1
        .recover_pubkey(&prefilled_data, &signature, &message)
        .map_err(|err| Error::Secp256k1)?;
    let actual_pubkey_hash = {
        let mut pubkey_hash = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(pubkey.as_slice());
        hasher.finalize(&mut pubkey_hash);
        pubkey_hash
    };
    if pubkey_hash != actual_pubkey_hash[..20] {
        return Err(Error::WrongSignature);
    }
    Ok(())
}

fn verify_l2block(l2block: &L2Block, prev_global_state: &GlobalState) -> Result<Context, Error> {
    // check merkle proof
    let raw_block = l2block.raw();
    let prev_account_root: [u8; 32] = raw_block.prev_account().merkle_root().unpack();
    if prev_global_state.account().merkle_root().unpack() != prev_account_root {
        return Err(Error::InvalidPrevGlobalState);
    }
    let number: u64 = raw_block.number().unpack();
    if number != prev_global_state.block().count().unpack() {
        return Err(Error::InvalidPrevGlobalState);
    }
    // check merkle proof
    let proof: Bytes = l2block.proof().unpack();
    let merkle_proof = CompiledMerkleProof(proof.to_vec());
    let kv_pairs: BTreeMap<_, _> = l2block.inputs().into_iter()
        .map(|kv| {
            let k: [u8; 32] = kv.k().unpack();
            let v: [u8; 32] = kv.v().unpack();
            (k.into(), v.into())
        })
        .collect();
    if !merkle_proof
        .verify::<Blake2bHasher>(&prev_account_root.into(), kv_pairs.iter().map(|(k, v)| (*k, *v)).collect())
        .map_err(|_| Error::MerkleVerify)?
    {
        return Err(Error::InvalidMerkleProof);
    }
    let aggregator_id: u32 = raw_block.aggregator_id().unpack();
    // verify aggregator
    verify_aggregator(aggregator_id)?;

    let account_count: u32 = raw_block.prev_account().count().unpack();
    let rollup_type_id = load_script_hash()?;
    let context = Context {
        number,
        aggregator_id,
        kv_pairs,
        account_count,
        rollup_type_id,
    };
    Ok(context)
}

pub fn main() -> Result<(), Error> {
    // Initialize CKBDLContext
    let mut context = CKBDLContext::<[u8; 128 * 1024]>::new();
    let lib_secp256k1 = LibSecp256k1::load(&mut context);
    // basic verification
    let prev_global_state = parse_global_state(Source::GroupInput)?;
    let l2block = parse_l2block()?;
    let mut context = verify_l2block(&l2block, &prev_global_state)?;
    let raw_block = l2block.raw();
    // check signature
    verify_block_signature(&context, &lib_secp256k1, &l2block)?;

    // handle state transitions
    actions::join::handle_join(&mut context, &raw_block)?;
    let script = load_script()?;
    let args: Bytes = script.args().unpack();
    debug!("script args is {:?}", args);

    let tx_hash = load_tx_hash()?;
    debug!("tx hash is {:?}", tx_hash);

    let _buf: Vec<_> = vec![0u8; 32];

    Ok(())
}
