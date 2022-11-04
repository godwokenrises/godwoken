use crate::verifications::context::{verify_tx_context, TxContext, TxContextInput};
use core::result::Result;
use gw_state::{ckb_smt::smt::Pair, constants::GW_MAX_KV_PAIRS, kv_state::KVState};
use gw_types::{
    packed::{ChallengeLockArgs, RollupConfig},
    prelude::*,
};
use gw_utils::{
    cells::{rollup::MAX_ROLLUP_WITNESS_SIZE, utils::search_lock_hash},
    ckb_std::{ckb_constants::Source, ckb_types::bytes::Bytes, debug},
    error::Error,
    gw_types::packed::{BytesReader, CCTransactionWitnessReader, WitnessArgsReader},
};
use gw_utils::{ckb_std::syscalls::load_witness, gw_types};

/// Verify tx execution
pub fn verify_tx_execution(
    rollup_config: &RollupConfig,
    lock_args: &ChallengeLockArgs,
) -> Result<(), Error> {
    let mut buf = [0u8; MAX_ROLLUP_WITNESS_SIZE];
    let loaded_len = load_witness(&mut buf, 0, 0, Source::GroupInput)?;
    debug!("verity tx execution witness, loaded len: {}", loaded_len);

    let witness_args: BytesReader = {
        let reader = WitnessArgsReader::from_slice(&buf[..loaded_len]).map_err(|_err| {
            debug!("witness is not a valid WitnessArgsReader");
            Error::Encoding
        })?;

        reader.lock().to_opt().ok_or(Error::InvalidArgs)?
    };

    let unlock_args = match CCTransactionWitnessReader::verify(witness_args.raw_data(), false) {
        Ok(_) => CCTransactionWitnessReader::new_unchecked(witness_args.raw_data()),
        Err(_) => return Err(Error::InvalidArgs),
    };

    let tx = unlock_args.l2tx().to_entity();
    let mut tree_buffer = [Pair::default(); GW_MAX_KV_PAIRS];
    let kv_state_proof: Bytes = unlock_args.kv_state_proof().unpack();
    let kv_state = KVState::build(
        &mut tree_buffer,
        unlock_args.kv_state(),
        &kv_state_proof,
        unlock_args.account_count().unpack(),
        None,
    )?;
    let scripts = unlock_args.scripts().to_entity();
    let raw_block = unlock_args.raw_l2block().to_entity();
    let target = lock_args.target();
    let tx_proof = unlock_args.tx_proof().to_entity();

    let input = TxContextInput {
        tx,
        kv_state,
        scripts,
        raw_block,
        rollup_config,
        target,
        tx_proof,
    };

    let TxContext {
        receiver_script_hash,
        ..
    } = verify_tx_context(input)?;

    // verify backend script is in the input
    // the backend will do the post state verification
    if search_lock_hash(&receiver_script_hash.into(), Source::Input).is_none() {
        debug!(
            "verify tx execution, can't find receiver_script_hash from the input: {:?}",
            &receiver_script_hash
        );
        return Err(Error::AccountScriptCellNotFound);
    }

    Ok(())
}
