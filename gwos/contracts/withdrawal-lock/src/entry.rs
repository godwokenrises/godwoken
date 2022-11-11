// Import from `core` instead of from `std` since we are in no-std mode
use core::result::Result;

use gw_types::{
    packed::{UnlockWithdrawalWitness, UnlockWithdrawalWitnessReader},
    prelude::*,
};
use gw_utils::cells::rollup::{
    load_rollup_config, parse_rollup_action, search_rollup_cell, search_rollup_state,
};
use gw_utils::ckb_std::{
    debug,
    high_level::{load_cell_lock_hash, QueryIter},
};
use gw_utils::finality::is_finalized;
use gw_utils::gw_types::packed::{
    CustodianLockArgs, CustodianLockArgsReader, RollupActionUnionReader,
    UnlockWithdrawalWitnessUnion, WithdrawalLockArgs,
};
use gw_utils::{
    cells::rollup::MAX_ROLLUP_WITNESS_SIZE,
    gw_types::{self, core::ScriptHashType},
    Timepoint,
};
use gw_utils::{cells::utils::search_lock_hash, ckb_std::high_level::load_cell_lock};

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use crate::ckb_std::{
    ckb_constants::Source,
    ckb_types::{self, bytes::Bytes, prelude::Unpack as CKBUnpack},
    high_level::{
        load_cell_capacity, load_cell_data, load_cell_type_hash, load_script, load_witness_args,
    },
};

use crate::error::Error;

const FINALIZED_BLOCK_TIMEPOINT: u64 = 0;
const FINALIZED_BLOCK_HASH: [u8; 32] = [0u8; 32];

struct ParsedLockArgs {
    rollup_type_hash: [u8; 32],
    lock_args: WithdrawalLockArgs,
    owner_lock_hash: [u8; 32],
}

/// args: rollup_type_hash | withdrawal lock args | owner lock len (optional) | owner lock (optional)
fn parse_lock_args(script: &ckb_types::packed::Script) -> Result<ParsedLockArgs, Error> {
    let mut rollup_type_hash = [0u8; 32];
    let args: Bytes = script.args().unpack();
    if args.len() < rollup_type_hash.len() {
        return Err(Error::InvalidArgs);
    }

    rollup_type_hash.copy_from_slice(&args[..32]);
    let parsed = gw_utils::withdrawal::parse_lock_args(&args)?;

    Ok(ParsedLockArgs {
        rollup_type_hash,
        lock_args: parsed.lock_args,
        owner_lock_hash: parsed.owner_lock.hash(),
    })
}

pub fn main() -> Result<(), Error> {
    let script = load_script()?;
    let ParsedLockArgs {
        rollup_type_hash,
        lock_args,
        owner_lock_hash,
    } = parse_lock_args(&script)?;

    // load unlock arguments from witness
    let witness_args = load_witness_args(0, Source::GroupInput)?;
    let unlock_args = {
        let unlock_args: Bytes = witness_args
            .lock()
            .to_opt()
            .ok_or(Error::InvalidArgs)?
            .unpack();
        match UnlockWithdrawalWitnessReader::verify(&unlock_args, false) {
            Ok(()) => UnlockWithdrawalWitness::new_unchecked(unlock_args),
            Err(_) => return Err(Error::ProofNotFound),
        }
    };

    // execute verification
    match unlock_args.to_enum() {
        UnlockWithdrawalWitnessUnion::UnlockWithdrawalViaRevert(unlock_args) => {
            let mut rollup_action_witness = [0u8; MAX_ROLLUP_WITNESS_SIZE];
            let withdrawal_block_hash = lock_args.withdrawal_block_hash();
            // prove the block is reverted
            let rollup_action = {
                let index = search_rollup_cell(&rollup_type_hash, Source::Output)
                    .ok_or(Error::RollupCellNotFound)?;
                parse_rollup_action(&mut rollup_action_witness, index, Source::Output)?
            };
            match rollup_action.to_enum() {
                RollupActionUnionReader::RollupSubmitBlock(args) => {
                    if !args
                        .reverted_block_hashes()
                        .iter()
                        .any(|hash| hash.as_slice() == withdrawal_block_hash.as_slice())
                    {
                        return Err(Error::InvalidRevertedBlocks);
                    }
                }
                _ => {
                    return Err(Error::InvalidRevertedBlocks);
                }
            }
            let custodian_lock_hash: [u8; 32] = unlock_args.custodian_lock_hash().unpack();
            // check there are a reverted custodian lock in the output
            let custodian_cell_index = match search_lock_hash(&custodian_lock_hash, Source::Output)
            {
                Some(index) => index,
                None => return Err(Error::InvalidOutput),
            };

            // check reverted custodian deposit info.
            let custodian_lock = load_cell_lock(custodian_cell_index, Source::Output)?;
            let custodian_lock_args = {
                let args: Bytes = custodian_lock.args().unpack();
                if args.len() < rollup_type_hash.len() {
                    return Err(Error::InvalidArgs);
                }
                if args[..32] != rollup_type_hash {
                    return Err(Error::InvalidArgs);
                }

                match CustodianLockArgsReader::verify(&args.slice(32..), false) {
                    Ok(_) => CustodianLockArgs::new_unchecked(args.slice(32..)),
                    Err(_) => return Err(Error::InvalidOutput),
                }
            };
            let custodian_deposit_block_hash: [u8; 32] =
                custodian_lock_args.deposit_block_hash().unpack();
            let custodian_deposit_block_timepoint: u64 =
                custodian_lock_args.deposit_block_number().unpack();
            let global_state = search_rollup_state(&rollup_type_hash, Source::Input)?
                .ok_or(Error::RollupCellNotFound)?;
            let config = load_rollup_config(&global_state.rollup_config_hash().unpack())?;
            if custodian_lock.code_hash().as_slice()
                != config.custodian_script_type_hash().as_slice()
                || custodian_lock.hash_type() != ScriptHashType::Type.into()
                || custodian_deposit_block_hash != FINALIZED_BLOCK_HASH
                || custodian_deposit_block_timepoint != FINALIZED_BLOCK_TIMEPOINT
            {
                return Err(Error::InvalidOutput);
            }

            // check capacity, data_hash, type_hash
            check_output_cell_has_same_content(0, Source::GroupInput, custodian_cell_index)?;
            Ok(())
        }
        UnlockWithdrawalWitnessUnion::UnlockWithdrawalViaFinalize(_unlock_args) => {
            // try search rollup state from deps
            let global_state = match search_rollup_state(&rollup_type_hash, Source::CellDep)? {
                Some(state) => state,
                None => {
                    // then try search rollup state from inputs
                    search_rollup_state(&rollup_type_hash, Source::Input)?
                        .ok_or(Error::RollupCellNotFound)?
                }
            };
            let config = load_rollup_config(&global_state.rollup_config_hash().unpack())?;

            // check finality
            let is_finalized = is_finalized(
                &config,
                &global_state,
                &Timepoint::from_full_value(lock_args.withdrawal_block_number().unpack()),
            );
            if !is_finalized {
                return Err(Error::NotFinalized);
            }

            // withdrawal lock is finalized, unlock for owner
            {
                // check whether output cell at same index only change lock script
                let withdrawal_lock_hash = load_cell_lock_hash(0, Source::GroupInput)?;

                let mut invalid_output_found = false;
                for (index, _) in QueryIter::new(load_cell_lock_hash, Source::Input)
                    .enumerate()
                    .filter(|(_idx, lock_hash)| lock_hash == &withdrawal_lock_hash)
                {
                    if check_output_cell_has_same_content(index, Source::Input, index).is_err() {
                        debug!("[via finalize] output cell content not match, fallback to input owner cell");
                        invalid_output_found = true;
                        break;
                    }

                    let maybe_output_lock_hash = load_cell_lock_hash(index, Source::Output);
                    if maybe_output_lock_hash != Ok(owner_lock_hash) {
                        debug!("[via finalize] output cell owner lock not match, fallback to input owner cell");
                        invalid_output_found = true;
                        break;
                    }
                }

                if !invalid_output_found {
                    return Ok(());
                }
            }

            // fallback to input owner cell way
            if search_lock_hash(&lock_args.owner_lock_hash().unpack(), Source::Input).is_none() {
                return Err(Error::OwnerCellNotFound);
            }

            Ok(())
        }
    }
}

fn check_output_cell_has_same_content(
    input_index: usize,
    input_source: Source,
    output_index: usize,
) -> Result<(), Error> {
    if load_cell_capacity(input_index, input_source)?
        != load_cell_capacity(output_index, Source::Output)?
    {
        return Err(Error::InvalidOutput);
    }

    // TODO: use load_cell_data_hash
    // NOTE: load_cell_data_hash from inputs throw ItemMissing error. Comparing data directly
    // as temporary workaround. Right now data should be sudt amount only, 16 bytes long.
    if load_cell_data(input_index, input_source)? != load_cell_data(output_index, Source::Output)? {
        return Err(Error::InvalidOutput);
    }

    if load_cell_type_hash(input_index, input_source)?
        != load_cell_type_hash(output_index, Source::Output)?
    {
        return Err(Error::InvalidOutput);
    }
    Ok(())
}
