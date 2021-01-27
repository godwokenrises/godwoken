// Import from `core` instead of from `std` since we are in no-std mode
use core::result::Result;

// Import heap related library from `alloc`
// https://doc.rust-lang.org/alloc/index.html
use alloc::vec;
use validator_utils::{
    ckb_std::{
        ckb_constants::Source,
        ckb_types::{bytes::Bytes, prelude::Unpack as CKBUnpack},
        high_level::{load_input_since, load_script, load_witness_args},
        since::{LockValue, Since},
    },
    error::Error,
    kv_state::KVState,
    search_cells::{parse_rollup_action, search_lock_hash, search_rollup_cell},
    signature::check_input_account_lock,
};

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use gw_common::{
    h256_ext::H256Ext,
    merkle_utils::calculate_compacted_account_root,
    smt::{Blake2bHasher, CompiledMerkleProof},
    state::State,
    H256,
};
use gw_types::{packed::{CancelChallenge, CancelChallengeReader, ChallengeLockArgs, ChallengeLockArgsReader, RollupActionUnion, RollupCancelChallenge, RollupConfig}, prelude::*};

/// args: rollup_type_hash | start challenge
fn parse_lock_args() -> Result<([u8; 32], ChallengeLockArgs), Error> {
    let script = load_script()?;
    let args: Bytes = script.args().unpack();

    let mut rollup_type_hash: [u8; 32] = [0u8; 32];
    if args.len() < rollup_type_hash.len() {
        return Err(Error::InvalidArgs);
    }
    rollup_type_hash.copy_from_slice(&args[..32]);
    match ChallengeLockArgsReader::verify(&args.slice(32..), false) {
        Ok(()) => Ok((
            rollup_type_hash,
            ChallengeLockArgs::new_unchecked(args.slice(32..)),
        )),
        Err(_) => Err(Error::InvalidArgs),
    }
}

/// args:
/// * rollup_script_hash | ChallengeLockArgs
///
/// unlock paths:
/// * challenge success unlock
///   * the cell is generated at least CHALLENGE_MUTURATY blocks
/// * cancel challenge unlock
///   * a cancel challenge tx is sent to consume this cell
///   * a backend verifier cell in the inputs
///   * the verification context of backend verifier is correct
pub fn main() -> Result<(), Error> {
    let (rollup_script_hash, lock_args) = parse_lock_args()?;
    // check rollup cell
    let index =
        search_rollup_cell(&rollup_script_hash, Source::Output).ok_or(Error::RollupCellNotFound)?;
    let action = parse_rollup_action(index, Source::Output)?;
    match action.to_enum() {
        RollupActionUnion::RollupEnterChallenge(_) | RollupActionUnion::RollupRevert(_) => {
            // state-validator will do the verification
            return Ok(());
        }
        RollupActionUnion::RollupCancelChallenge(_) => {}
        _ => {
            return Err(Error::InvalidArgs);
        }
    }

    // unlock via cancel challenge
    let witness_args: Bytes = load_witness_args(0, Source::GroupInput)?
        .lock()
        .to_opt()
        .ok_or(Error::InvalidArgs)?
        .unpack();
    let unlock_args = match CancelChallengeReader::verify(&witness_args, false) {
        Ok(_) => CancelChallenge::new_unchecked(witness_args),
        Err(_) => return Err(Error::InvalidArgs),
    };

    // verify tx signature
    let tx = unlock_args.l2tx();
    let raw_tx = tx.raw();
    let tx_hash = raw_tx.hash();
    let account_count: u32 = unlock_args.account_count().unpack();
    let kv_state = KVState::new(
        unlock_args.kv_state(),
        unlock_args.kv_state_proof().unpack(),
        account_count,
    );
    let sender_script_hash = kv_state
        .get_script_hash(raw_tx.from_id().unpack())
        .map_err(|_| Error::SMTKeyMissing)?;
    check_input_account_lock(sender_script_hash, tx_hash.into())?;

    // verify backend script is in the input
    let script_hash = kv_state
        .get_script_hash(raw_tx.to_id().unpack())
        .map_err(|_| Error::SMTKeyMissing)?;
    // the backend will do the post state verification
    if search_lock_hash(&script_hash.into(), Source::Input).is_none() {
        return Err(Error::InvalidOutput);
    }

    // verify block hash
    let raw_block = unlock_args.raw_l2block();
    if &raw_block.hash() != lock_args.target().block_hash().as_slice() {
        return Err(Error::InvalidOutput);
    }

    // verify tx
    let tx_witness_root: [u8; 32] = raw_block.submit_transactions().tx_witness_root().unpack();
    let tx_index: u32 = lock_args.target().tx_index().unpack();
    let tx_witness_hash: [u8; 32] = tx.witness_hash();
    let valid = CompiledMerkleProof(unlock_args.tx_proof().unpack())
        .verify::<Blake2bHasher>(
            &tx_witness_root.into(),
            vec![(H256::from_u32(tx_index), tx_witness_hash.into())],
        )
        .map_err(|_| Error::MerkleProof)?;
    if !valid {
        return Err(Error::MerkleProof);
    }

    // verify prev state root
    let prev_compacted_root: [u8; 32] = raw_block
        .submit_transactions()
        .compacted_post_root_list()
        .get(tx_index as usize)
        .ok_or(Error::InvalidArgs)?
        .unpack();
    let state_root = kv_state.calculate_root().map_err(|_| Error::MerkleProof)?;
    let calculated_compacted_root =
        calculate_compacted_account_root(&state_root.into(), account_count);
    if prev_compacted_root != calculated_compacted_root {
        return Err(Error::MerkleProof);
    }

    Ok(())
}
