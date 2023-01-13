//! This module test consume challenge-resolve cells without challenge context
//!
//! Because of anyone can send challenge-resolve context to resolve a challenge,
//! once the challenge gets resolved, others need to be able to consume thier
//! challenge resolve cell and get CKB back.

use ckb_script::TransactionScriptsVerifier;
use gw_types::{
    bytes::Bytes,
    packed::{CellDep, CellInput, CellOutput},
    prelude::*,
};

use crate::{
    script_tests::{
        programs::{META_CONTRACT_CODE_HASH, META_CONTRACT_VALIDATOR_PROGRAM},
        utils::layer1::{
            build_resolved_tx, build_simple_tx_with_out_point, random_out_point, DummyDataLoader,
            MAX_CYCLES,
        },
    },
    testing_tool::chain::{ALWAYS_SUCCESS_CODE_HASH, ALWAYS_SUCCESS_PROGRAM},
};

#[test]
fn test_consume_challenge_resolve_cell() {
    // deploy scripts
    let mut ctx = DummyDataLoader::default();
    let script_out_point = random_out_point();
    ctx.cells.insert(
        script_out_point.clone(),
        (
            CellOutput::default(),
            META_CONTRACT_VALIDATOR_PROGRAM.clone(),
        ),
    );

    let lock_out_point = random_out_point();
    ctx.cells.insert(
        lock_out_point.clone(),
        (CellOutput::default(), ALWAYS_SUCCESS_PROGRAM.clone()),
    );

    let (owner_lock_out_point, owner_lock_hash) = {
        let cell = CellOutput::new_builder()
            .lock(
                ckb_types::packed::Script::new_builder()
                    .code_hash(Pack::pack(&*ALWAYS_SUCCESS_CODE_HASH))
                    .hash_type(gw_types::core::ScriptHashType::Data.into())
                    .build(),
            )
            .capacity(Pack::pack(&42u64))
            .build();
        let owner_lock_hash: [u8; 32] = cell.lock().calc_script_hash().unpack();
        let owner_lock_out_point = random_out_point();
        ctx.cells
            .insert(owner_lock_out_point.clone(), (cell, Bytes::default()));
        (owner_lock_out_point, owner_lock_hash)
    };

    let (challenge_resolve_cell, data) = {
        // mocked rollup script hash
        let rollup_script_hash = [42u8; 32];
        let args = Bytes::from(rollup_script_hash.to_vec());
        let cell = CellOutput::new_builder()
            .lock(
                ckb_types::packed::Script::new_builder()
                    .code_hash(Pack::pack(&*META_CONTRACT_CODE_HASH))
                    .hash_type(gw_types::core::ScriptHashType::Data.into())
                    .args(Pack::pack(&args))
                    .build(),
            )
            .capacity(Pack::pack(&42u64))
            .build();
        (cell, Bytes::from(owner_lock_hash.to_vec()))
    };
    let challenge_resolved_out_point = random_out_point();

    let tx = build_simple_tx_with_out_point(
        &mut ctx,
        (challenge_resolve_cell, data),
        challenge_resolved_out_point,
        (CellOutput::default(), Bytes::default()),
    )
    .as_advanced_builder()
    .witness(Pack::pack(&Bytes::default()))
    .input(
        CellInput::new_builder()
            .previous_output(owner_lock_out_point)
            .build(),
    )
    .witness(Pack::pack(&Bytes::default()))
    .cell_dep(CellDep::new_builder().out_point(script_out_point).build())
    .cell_dep(CellDep::new_builder().out_point(lock_out_point).build())
    .build();
    let resolved_tx = build_resolved_tx(&ctx, &tx);
    let mut verifier = TransactionScriptsVerifier::new(&resolved_tx, &ctx);
    verifier.set_debug_printer(|_script, msg| println!("[script debug] {}", msg));
    verifier.verify(MAX_CYCLES).expect("success");
}
