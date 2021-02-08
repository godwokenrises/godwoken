use crate::tests::utils::layer1::{
    always_success_script, build_resolved_tx, random_out_point, DummyDataLoader,
    ALWAYS_SUCCESS_CODE_HASH, ALWAYS_SUCCESS_PROGRAM, CHALLENGE_LOCK_PROGRAM, MAX_CYCLES,
};
use ckb_script::TransactionScriptsVerifier;
use ckb_types::{
    packed::{CellDep, CellOutput},
    prelude::Pack as CKBPack,
};
use gw_common::blake2b::new_blake2b;
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{RollupConfig, StakeLockArgs},
    prelude::*,
};
use lazy_static::lazy_static;
use std::{fs, io::Read, path::PathBuf};

mod cancel_challenge;
mod enter_challenge;
mod revert;
mod submit_block;

const SCRIPT_DIR: &'static str = "../../build/debug";
const STATE_VALIDATOR: &'static str = "state-validator";

lazy_static! {
    pub static ref STATE_VALIDATOR_PROGRAM: Bytes = {
        let mut buf = Vec::new();
        let mut path = PathBuf::new();
        path.push(&SCRIPT_DIR);
        path.push(&STATE_VALIDATOR);
        let mut f = fs::File::open(&path).expect("load program");
        f.read_to_end(&mut buf).expect("read program");
        Bytes::from(buf.to_vec())
    };
    pub static ref STATE_VALIDATOR_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&STATE_VALIDATOR_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
}

#[derive(Default)]
pub struct CellContextParam {
    stake_lock_type: ckb_types::packed::Script,
    challenge_lock_type: ckb_types::packed::Script,
    deposit_lock_type: ckb_types::packed::Script,
    custodian_lock_type: ckb_types::packed::Script,
    withdrawal_lock_type: ckb_types::packed::Script,
}

pub struct CellContext {
    inner: DummyDataLoader,
    state_validator_dep: CellDep,
    rollup_config_dep: CellDep,
    stake_lock_dep: CellDep,
    challenge_lock_dep: CellDep,
    deposit_lock_dep: CellDep,
    custodian_lock_dep: CellDep,
    withdrawal_lock_dep: CellDep,
    always_success_dep: CellDep,
}

impl CellContext {
    pub fn new(rollup_config: &RollupConfig, param: CellContextParam) -> Self {
        let mut data_loader = DummyDataLoader::default();
        let always_success_dep = {
            let always_success_out_point = random_out_point();
            data_loader.cells.insert(
                always_success_out_point.clone(),
                (
                    CellOutput::new_builder()
                        .capacity(CKBPack::pack(&(ALWAYS_SUCCESS_PROGRAM.len() as u64)))
                        .build(),
                    ALWAYS_SUCCESS_PROGRAM.clone(),
                ),
            );
            CellDep::new_builder()
                .out_point(always_success_out_point)
                .build()
        };
        let state_validator_dep = {
            let state_validator_out_point = random_out_point();
            data_loader.cells.insert(
                state_validator_out_point.clone(),
                (
                    CellOutput::new_builder()
                        .capacity(CKBPack::pack(&(STATE_VALIDATOR_PROGRAM.len() as u64)))
                        .build(),
                    STATE_VALIDATOR_PROGRAM.clone(),
                ),
            );
            CellDep::new_builder()
                .out_point(state_validator_out_point)
                .build()
        };
        let rollup_config_dep = {
            let rollup_config_out_point = random_out_point();
            data_loader.cells.insert(
                rollup_config_out_point.clone(),
                (
                    CellOutput::new_builder()
                        .capacity(CKBPack::pack(&(rollup_config.as_bytes().len() as u64)))
                        .build(),
                    rollup_config.as_bytes(),
                ),
            );
            CellDep::new_builder()
                .out_point(rollup_config_out_point)
                .build()
        };
        let stake_lock_dep = {
            let stake_out_point = random_out_point();
            data_loader.cells.insert(
                stake_out_point.clone(),
                (
                    CellOutput::new_builder()
                        .capacity(CKBPack::pack(&(ALWAYS_SUCCESS_PROGRAM.len() as u64)))
                        .type_(CKBPack::pack(&Some(param.stake_lock_type.clone())))
                        .build(),
                    ALWAYS_SUCCESS_PROGRAM.clone(),
                ),
            );
            CellDep::new_builder().out_point(stake_out_point).build()
        };
        let challenge_lock_dep = {
            let out_point = random_out_point();
            data_loader.cells.insert(
                out_point.clone(),
                (
                    CellOutput::new_builder()
                        .capacity(CKBPack::pack(&(CHALLENGE_LOCK_PROGRAM.len() as u64)))
                        .type_(CKBPack::pack(&Some(param.challenge_lock_type.clone())))
                        .build(),
                    CHALLENGE_LOCK_PROGRAM.clone(),
                ),
            );
            CellDep::new_builder().out_point(out_point).build()
        };
        let deposit_lock_dep = {
            let out_point = random_out_point();
            data_loader.cells.insert(
                out_point.clone(),
                (
                    CellOutput::new_builder()
                        .capacity(CKBPack::pack(&(ALWAYS_SUCCESS_PROGRAM.len() as u64)))
                        .type_(CKBPack::pack(&Some(param.deposit_lock_type.clone())))
                        .build(),
                    ALWAYS_SUCCESS_PROGRAM.clone(),
                ),
            );
            CellDep::new_builder().out_point(out_point).build()
        };
        let custodian_lock_dep = {
            let out_point = random_out_point();
            data_loader.cells.insert(
                out_point.clone(),
                (
                    CellOutput::new_builder()
                        .capacity(CKBPack::pack(&(ALWAYS_SUCCESS_PROGRAM.len() as u64)))
                        .type_(CKBPack::pack(&Some(param.custodian_lock_type.clone())))
                        .build(),
                    ALWAYS_SUCCESS_PROGRAM.clone(),
                ),
            );
            CellDep::new_builder().out_point(out_point).build()
        };
        let withdrawal_lock_dep = {
            let out_point = random_out_point();
            data_loader.cells.insert(
                out_point.clone(),
                (
                    CellOutput::new_builder()
                        .capacity(CKBPack::pack(&(ALWAYS_SUCCESS_PROGRAM.len() as u64)))
                        .type_(CKBPack::pack(&Some(param.withdrawal_lock_type.clone())))
                        .build(),
                    ALWAYS_SUCCESS_PROGRAM.clone(),
                ),
            );
            CellDep::new_builder().out_point(out_point).build()
        };
        CellContext {
            inner: data_loader,
            rollup_config_dep,
            always_success_dep,
            stake_lock_dep,
            state_validator_dep,
            challenge_lock_dep,
            deposit_lock_dep,
            custodian_lock_dep,
            withdrawal_lock_dep,
        }
    }

    pub fn insert_cell(
        &mut self,
        cell: ckb_types::packed::CellOutput,
        data: Bytes,
    ) -> ckb_types::packed::OutPoint {
        let out_point = random_out_point();
        self.inner.cells.insert(out_point.clone(), (cell, data));
        out_point
    }

    pub fn verify_tx(
        &self,
        tx: ckb_types::core::TransactionView,
    ) -> Result<ckb_types::core::Cycle, ckb_error::Error> {
        let resolved_tx = build_resolved_tx(&self.inner, &tx);
        let mut verifier = TransactionScriptsVerifier::new(&resolved_tx, &self.inner);
        verifier.set_debug_printer(|_script, msg| println!("[script debug] {}", msg));
        verifier.verify(MAX_CYCLES)
    }
}

pub fn state_validator_script() -> ckb_types::packed::Script {
    ckb_types::packed::Script::new_builder()
        .code_hash(CKBPack::pack(&*STATE_VALIDATOR_CODE_HASH))
        .hash_type(ScriptHashType::Data.into())
        .build()
}

pub fn build_type_id_script(name: &[u8]) -> ckb_types::packed::Script {
    ckb_types::packed::Script::new_builder()
        .code_hash(CKBPack::pack(&ALWAYS_SUCCESS_CODE_HASH.clone()))
        .args(CKBPack::pack(&Bytes::from(name.to_vec())))
        .build()
}

pub fn build_rollup_locked_cell(
    rollup_type_script_hash: &[u8; 32],
    script_type_hash: &[u8; 32],
    capacity: u64,
    lock_args: Bytes,
) -> ckb_types::packed::CellOutput {
    let lock = {
        let mut args = Vec::new();
        args.extend_from_slice(rollup_type_script_hash);
        args.extend_from_slice(&lock_args);
        ckb_types::packed::Script::new_builder()
            .code_hash(CKBPack::pack(script_type_hash))
            .hash_type(ScriptHashType::Type.into())
            .args(CKBPack::pack(&Bytes::from(args)))
            .build()
    };
    CellOutput::new_builder()
        .lock(lock)
        .capacity(CKBPack::pack(&capacity))
        .build()
}

pub fn build_always_success_cell(
    capacity: u64,
    type_: Option<ckb_types::packed::Script>,
) -> ckb_types::packed::CellOutput {
    CellOutput::new_builder()
        .lock(always_success_script())
        .type_(CKBPack::pack(&type_))
        .capacity(CKBPack::pack(&capacity))
        .build()
}
