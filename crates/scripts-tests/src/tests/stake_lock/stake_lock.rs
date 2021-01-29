use super::{STAKE_LOCK_CODE_HASH, STAKE_LOCK_PROGRAM};
use crate::tests::utils::{
    layer1::{
        always_success_script, build_resolved_tx, build_simple_tx, random_out_point,
        DummyDataLoader, ALWAYS_SUCCESS_CODE_HASH, ALWAYS_SUCCESS_PROGRAM, MAX_CYCLES,
        STATE_VALIDATOR_CODE_HASH, STATE_VALIDATOR_PROGRAM,
    },
    layer2::setup_chain,
};
use ckb_script::TransactionScriptsVerifier;
use ckb_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{Byte32, CellDep, CellInput, CellOutput, Script, Transaction},
    prelude::*,
};
use gw_chain::{
    chain::{Chain, ProduceBlockParam},
    mem_pool::{MemPool, PackageParam},
    next_block_context::NextBlockContext,
};
use gw_config::{ChainConfig, GenesisConfig};
use gw_generator::{
    account_lock_manage::{always_success::AlwaysSuccess, AccountLockManage},
    backend_manage::BackendManage,
    genesis::init_genesis,
    Generator,
};
use gw_store::Store;
use gw_types::packed::StakeLockArgs;
use parking_lot::Mutex;
use std::sync::Arc;

fn stake_lock_script(args: Bytes) -> Script {
    Script::new_builder()
        .code_hash(STAKE_LOCK_CODE_HASH.pack())
        .hash_type(ScriptHashType::Data.into())
        .args(args.pack())
        .build()
}

fn state_validator_script() -> Script {
    Script::new_builder()
        .code_hash(STATE_VALIDATOR_CODE_HASH.pack())
        .hash_type(ScriptHashType::Data.into())
        .build()
}

#[test]
fn test_unlock_stake_lock_by_owner_lock_hash() {
    let mut data_loader = DummyDataLoader::default();

    // deploy scripts
    let state_validator_dep = {
        let state_validator_out_point = random_out_point();
        data_loader.cells.insert(
            state_validator_out_point.clone(),
            (
                CellOutput::new_builder()
                    .capacity((STATE_VALIDATOR_PROGRAM.len() as u64).pack())
                    .build(),
                STATE_VALIDATOR_PROGRAM.clone(),
            ),
        );
        CellDep::new_builder()
            .out_point(state_validator_out_point)
            .build()
    };
    let always_success_dep = {
        let always_success_out_point = random_out_point();
        data_loader.cells.insert(
            always_success_out_point.clone(),
            (
                CellOutput::new_builder()
                    .capacity((ALWAYS_SUCCESS_PROGRAM.len() as u64).pack())
                    .build(),
                ALWAYS_SUCCESS_PROGRAM.clone(),
            ),
        );
        CellDep::new_builder()
            .out_point(always_success_out_point)
            .build()
    };
    let stake_lock_dep = {
        let stake_lock_out_point = random_out_point();
        data_loader.cells.insert(
            stake_lock_out_point.clone(),
            (
                CellOutput::new_builder()
                    .capacity((STAKE_LOCK_PROGRAM.len() as u64).pack())
                    .build(),
                STAKE_LOCK_PROGRAM.clone(),
            ),
        );
        CellDep::new_builder()
            .out_point(stake_lock_out_point)
            .build()
    };
    // init chain and create rollup cell
    {
        let rollup_type_script = {
            gw_types::packed::Script::new_builder()
                .code_hash(gw_types::prelude::Pack::pack(&*STATE_VALIDATOR_CODE_HASH))
                .hash_type(gw_types::core::ScriptHashType::Data.into())
                .build()
        };
        let chain = setup_chain(&rollup_type_script);
        let global_state = chain.local_state.last_global_state();
        println!("global_state: {}", global_state);
        let capacity = 1000_00000000u64;
        let out_point = random_out_point();
        let rollup_cell = CellOutput::new_builder()
            .lock(always_success_script())
            .type_(Some(state_validator_script()).pack())
            .capacity(capacity.pack())
            .build();
        data_loader
            .cells
            .insert(out_point, (rollup_cell, global_state.as_bytes()));
    }

    // create always success input
    let always_success_input = {
        let capacity = 1000_00000000u64;
        let input_cell = CellOutput::new_builder()
            .lock(always_success_script())
            .capacity(capacity.pack())
            .build();
        let out_point = random_out_point();
        data_loader
            .cells
            .insert(out_point.clone(), (input_cell, Default::default()));
        CellInput::new_builder().previous_output(out_point).build()
    };
    // create stake_lock input
    let stake_lock_input = {
        let capacity = 1000_00000000u64;
        let stake_lock_script = {
            let stake_lock_args = {
                let owner_lock_hash = {
                    let hash: Byte32 = always_success_script().calc_script_hash();
                    gw_types::packed::Byte32::from_slice(hash.as_slice())
                        .expect("Build gw_types::packed::Byte32 from slice")
                };
                StakeLockArgs::new_builder()
                    .stake_block_number(gw_types::prelude::Pack::pack(&0u64))
                    .owner_lock_hash(owner_lock_hash)
                    .build()
            };
            let stake_lock_args_slice: &[u8] = stake_lock_args.as_slice();
            let rollup_type_hash: Byte32 = state_validator_script().calc_script_hash();
            let rollup_type_hash_slice: &[u8] = rollup_type_hash.as_slice();
            let lock_args_slice = [rollup_type_hash_slice, stake_lock_args_slice].concat();
            Script::new_builder()
                .code_hash(STAKE_LOCK_CODE_HASH.pack())
                .hash_type(ScriptHashType::Data.into())
                .args(lock_args_slice.pack())
                .build()
        };
        let spend_stake_cell = CellOutput::new_builder()
            .lock(stake_lock_script)
            .capacity(capacity.pack())
            .build();
        let out_point = random_out_point();
        data_loader
            .cells
            .insert(out_point.clone(), (spend_stake_cell, Default::default()));
        CellInput::new_builder().previous_output(out_point).build()
    };
    // create output cell
    let output_cell = {
        let output_capacity = 1000_00000000u64 * 2 - 10000000u64;
        CellOutput::new_builder()
            .lock(always_success_script())
            .capacity(output_capacity.pack())
            .build()
    };
    let tx = Transaction::default()
        .as_advanced_builder()
        .input(always_success_input)
        .input(stake_lock_input)
        .output(output_cell)
        .cell_dep(always_success_dep)
        .cell_dep(state_validator_dep)
        .cell_dep(stake_lock_dep)
        .build();
    let resolved_tx = build_resolved_tx(&data_loader, &tx);
    let mut verifier = TransactionScriptsVerifier::new(&resolved_tx, &data_loader);
    verifier.set_debug_printer(|_script, msg| println!("[script debug] {}", msg));
    verifier.verify(MAX_CYCLES).expect("return success");
}

#[test]
fn test_unlock_stake_lock_by_rollup_cell() {}
