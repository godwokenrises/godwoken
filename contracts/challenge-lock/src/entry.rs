// Import from `core` instead of from `std` since we are in no-std mode
use core::{convert::TryInto, result::Result};

// Import heap related library from `alloc`
// https://doc.rust-lang.org/alloc/index.html
use alloc::vec;
use validator_utils::{
    ckb_std::{
        ckb_constants::Source,
        ckb_types::{bytes::Bytes, prelude::Unpack as CKBUnpack},
        high_level::{load_script, load_witness_args},
    },
    error::Error,
    kv_state::KVState,
    search_cells::{
        load_rollup_config, parse_rollup_action, search_lock_hash, search_rollup_cell,
        search_rollup_state,
    },
    signature::check_input_account_lock,
};

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use gw_common::{
    blake2b::new_blake2b,
    h256_ext::H256Ext,
    merkle_utils::calculate_compacted_account_root,
    smt::{Blake2bHasher, CompiledMerkleProof},
    state::State,
    H256,
};
use gw_types::{
    core::{ChallengeTargetType, ScriptHashType},
    packed::{
        ChallengeLockArgs, ChallengeLockArgsReader, RollupActionUnion, RollupConfig,
        VerifyTransactionWitness, VerifyTransactionWitnessReader, VerifyWithdrawalWitness,
        VerifyWithdrawalWitnessReader,
    },
    prelude::*,
};

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

/// Verify transaction
///
/// 1. check the signature of tx
/// 2. check the verifier backend script exists
/// 3. do other merkle proof verification
fn verify_transaction(
    rollup_script_hash: &[u8; 32],
    rollup_config: &RollupConfig,
    lock_args: &ChallengeLockArgs,
) -> Result<(), Error> {
    let witness_args: Bytes = load_witness_args(0, Source::GroupInput)?
        .lock()
        .to_opt()
        .ok_or(Error::InvalidArgs)?
        .unpack();
    let unlock_args = match VerifyTransactionWitnessReader::verify(&witness_args, false) {
        Ok(_) => VerifyTransactionWitness::new_unchecked(witness_args),
        Err(_) => return Err(Error::InvalidArgs),
    };
    let tx = unlock_args.l2tx();
    let raw_tx = tx.raw();
    let account_count: u32 = unlock_args.account_count().unpack();
    let kv_state = KVState::new(
        unlock_args.kv_state(),
        unlock_args.kv_state_proof().unpack(),
        account_count,
    );
    // verify tx account's script
    let sender_id: u32 = raw_tx.from_id().unpack();
    let receiver_id: u32 = raw_tx.to_id().unpack();
    let sender_script_hash = kv_state
        .get_script_hash(sender_id)
        .map_err(|_| Error::SMTKeyMissing)?;
    let receiver_script_hash = kv_state
        .get_script_hash(receiver_id)
        .map_err(|_| Error::SMTKeyMissing)?;

    // find scripts
    let scripts = unlock_args.scripts();
    let sender_script = scripts
        .clone()
        .into_iter()
        .find(|script| H256::from(script.hash()) == sender_script_hash)
        .ok_or(Error::ScriptNotFound)?;
    let receiver_script = scripts
        .into_iter()
        .find(|script| H256::from(script.hash()) == receiver_script_hash)
        .ok_or(Error::ScriptNotFound)?;

    // sender must be a valid External Owned Account
    if sender_script.hash_type() != ScriptHashType::Type.into() {
        return Err(Error::UnknownEOAScript);
    }
    if rollup_config
        .allowed_eoa_type_hashes()
        .into_iter()
        .find(|code_hash| code_hash == &sender_script.code_hash())
        .is_none()
    {
        return Err(Error::UnknownEOAScript);
    }
    // receiver must be a valid contract account
    if receiver_script.hash_type() != ScriptHashType::Type.into() {
        return Err(Error::UnknownContractScript);
    }
    if rollup_config
        .allowed_contract_type_hashes()
        .into_iter()
        .find(|code_hash| code_hash == &receiver_script.code_hash())
        .is_none()
    {
        return Err(Error::UnknownContractScript);
    }

    // verify tx signature
    let message = {
        let mut hasher = new_blake2b();
        hasher.update(rollup_script_hash);
        hasher.update(&sender_script_hash.as_slice());
        hasher.update(&receiver_script_hash.as_slice());
        hasher.update(raw_tx.as_slice());
        let mut message = [0u8; 32];
        hasher.finalize(&mut message);
        message
    };
    check_input_account_lock(sender_script_hash, message.into())?;

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
    let tx_index: u32 = lock_args.target().target_index().unpack();
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

/// Verify withdrawal signature
fn verify_withdrawal(
    rollup_script_hash: &[u8; 32],
    rollup_config: &RollupConfig,
    lock_args: &ChallengeLockArgs,
) -> Result<(), Error> {
    let witness_args: Bytes = load_witness_args(0, Source::GroupInput)?
        .lock()
        .to_opt()
        .ok_or(Error::InvalidArgs)?
        .unpack();
    let unlock_args = match VerifyWithdrawalWitnessReader::verify(&witness_args, false) {
        Ok(_) => VerifyWithdrawalWitness::new_unchecked(witness_args),
        Err(_) => return Err(Error::InvalidArgs),
    };

    let withdrawal = unlock_args.withdrawal_request();
    let raw_withdrawal = withdrawal.raw();
    let sender_script_hash = raw_withdrawal.account_script_hash().unpack();
    // check withdrawal account type
    let account_script = unlock_args.account_script();
    if H256::from(account_script.hash()) != sender_script_hash {
        return Err(Error::ScriptNotFound);
    }
    // withdrawal account must be a valid External Owned Account
    if account_script.hash_type() != ScriptHashType::Type.into() {
        return Err(Error::UnknownEOAScript);
    }
    if rollup_config
        .allowed_eoa_type_hashes()
        .into_iter()
        .find(|code_hash| code_hash == &account_script.code_hash())
        .is_none()
    {
        return Err(Error::UnknownEOAScript);
    }

    // verify withdrawal signature
    let message = {
        let mut hasher = new_blake2b();
        hasher.update(rollup_script_hash);
        hasher.update(raw_withdrawal.as_slice());
        let mut message = [0u8; 32];
        hasher.finalize(&mut message);
        message
    };
    check_input_account_lock(sender_script_hash, message.into())?;

    // verify block hash
    let raw_block = unlock_args.raw_l2block();
    if &raw_block.hash() != lock_args.target().block_hash().as_slice() {
        return Err(Error::InvalidOutput);
    }

    // verify witness root
    let withdrawal_witness_root: [u8; 32] = raw_block
        .submit_withdrawals()
        .withdrawal_witness_root()
        .unpack();
    let withdrawal_index: u32 = lock_args.target().target_index().unpack();
    let withdrawal_witness_hash: [u8; 32] = withdrawal.witness_hash();
    let valid = CompiledMerkleProof(unlock_args.withdrawal_proof().unpack())
        .verify::<Blake2bHasher>(
            &withdrawal_witness_root.into(),
            vec![(
                H256::from_u32(withdrawal_index),
                withdrawal_witness_hash.into(),
            )],
        )
        .map_err(|_| Error::MerkleProof)?;
    if !valid {
        return Err(Error::MerkleProof);
    }

    Ok(())
}

/// args:
/// * rollup_script_hash | ChallengeLockArgs
///
/// unlock paths:
/// * challenge success
///   * after CHALLENGE_MATURITY_BLOCKS, the submitter can cancel challenge and resume Rollup to running status
/// * cancel challenge by execute verification
///   * during Rollup halting and submitter can do verification on-chain and cancel the challenge
///   * the verificaiton tx must has a backend verifier cell in the inputs
///   * the verification tx must provides verification context
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

    // load rollup config
    let rollup_config = {
        let prev_global_state = search_rollup_state(&rollup_script_hash, Source::Input)?
            .ok_or(Error::RollupCellNotFound)?;
        load_rollup_config(&prev_global_state.rollup_config_hash().unpack())?
    };

    // unlock via cancel challenge
    let challenge_target = lock_args.target();
    let target_type: ChallengeTargetType = {
        let target_type: u8 = challenge_target.target_type().into();
        target_type.try_into().map_err(|_| Error::InvalidArgs)?
    };

    match target_type {
        ChallengeTargetType::Transaction => {
            verify_transaction(&rollup_script_hash, &rollup_config, &lock_args)?;
        }
        ChallengeTargetType::Withdrawal => {
            verify_withdrawal(&rollup_script_hash, &rollup_config, &lock_args)?;
        }
    }

    Ok(())
}
