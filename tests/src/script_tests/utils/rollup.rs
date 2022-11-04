use crate::script_tests::utils::layer1::{
    always_success_script, build_resolved_tx, random_out_point, DummyDataLoader, MAX_CYCLES,
};
use crate::testing_tool::programs::{
    ALWAYS_SUCCESS_CODE_HASH, ALWAYS_SUCCESS_PROGRAM, CHALLENGE_LOCK_PROGRAM,
    ETH_ACCOUNT_LOCK_PROGRAM, SECP256K1_DATA, STATE_VALIDATOR_PROGRAM,
};
use ckb_chain_spec::consensus::ConsensusBuilder;
use ckb_script::{TransactionScriptsVerifier, TxVerifyEnv};
use ckb_types::core::hardfork::HardForkSwitch;
use ckb_types::core::HeaderView;
use ckb_types::{
    packed::{CellDep, CellOutput},
    prelude::Pack as CKBPack,
};
use gw_ckb_hardfork::{GLOBAL_CURRENT_EPOCH_NUMBER, GLOBAL_HARDFORK_SWITCH};
use gw_common::blake2b::new_blake2b;
use gw_types::{bytes::Bytes, core::ScriptHashType, packed::RollupConfig, prelude::*};

use std::sync::atomic::Ordering;

pub struct CellContextParam {
    pub stake_lock_type: ckb_types::packed::Script,
    pub challenge_lock_type: ckb_types::packed::Script,
    pub deposit_lock_type: ckb_types::packed::Script,
    pub custodian_lock_type: ckb_types::packed::Script,
    pub withdrawal_lock_type: ckb_types::packed::Script,
    pub l2_sudt_type: ckb_types::packed::Script,
    pub always_success_type: ckb_types::packed::Script,
    pub eoa_lock_type: ckb_types::packed::Script,
    pub eth_lock_type: ckb_types::packed::Script,
}

impl Default for CellContextParam {
    fn default() -> Self {
        Self {
            stake_lock_type: random_type_id_script(),
            challenge_lock_type: random_type_id_script(),
            deposit_lock_type: random_type_id_script(),
            custodian_lock_type: random_type_id_script(),
            withdrawal_lock_type: random_type_id_script(),
            l2_sudt_type: random_type_id_script(),
            always_success_type: random_type_id_script(),
            eoa_lock_type: random_type_id_script(),
            eth_lock_type: random_type_id_script(),
        }
    }
}

pub struct CellContext {
    pub inner: DummyDataLoader,
    pub state_validator_dep: CellDep,
    pub rollup_config_dep: CellDep,
    pub stake_lock_dep: CellDep,
    pub challenge_lock_dep: CellDep,
    pub deposit_lock_dep: CellDep,
    pub custodian_lock_dep: CellDep,
    pub withdrawal_lock_dep: CellDep,
    pub always_success_dep: CellDep,
    pub l2_sudt_dep: CellDep,
    /// default EoA lock(always success)
    pub eoa_lock_dep: CellDep,
    /// Eth account lock
    pub eth_lock_dep: CellDep,
    pub secp256k1_data_dep: CellDep,
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
                        .type_(CKBPack::pack(&Some(param.always_success_type.clone())))
                        .build(),
                    ALWAYS_SUCCESS_PROGRAM.clone(),
                ),
            );
            CellDep::new_builder()
                .out_point(always_success_out_point)
                .build()
        };
        let secp256k1_data_dep = {
            let secp256k1_data_out_point = random_out_point();
            data_loader.cells.insert(
                secp256k1_data_out_point.clone(),
                (
                    CellOutput::new_builder()
                        .capacity(CKBPack::pack(&(SECP256K1_DATA.len() as u64)))
                        .build(),
                    SECP256K1_DATA.clone(),
                ),
            );
            CellDep::new_builder()
                .out_point(secp256k1_data_out_point)
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
        let eoa_lock_dep = {
            let eoa_lock_out_point = random_out_point();
            data_loader.cells.insert(
                eoa_lock_out_point.clone(),
                (
                    CellOutput::new_builder()
                        .capacity(CKBPack::pack(&(ALWAYS_SUCCESS_PROGRAM.len() as u64)))
                        .type_(CKBPack::pack(&Some(param.eoa_lock_type.clone())))
                        .build(),
                    ALWAYS_SUCCESS_PROGRAM.clone(),
                ),
            );
            CellDep::new_builder().out_point(eoa_lock_out_point).build()
        };
        let eth_lock_dep = {
            let eth_lock_out_point = random_out_point();
            data_loader.cells.insert(
                eth_lock_out_point.clone(),
                (
                    CellOutput::new_builder()
                        .capacity(CKBPack::pack(&(ETH_ACCOUNT_LOCK_PROGRAM.len() as u64)))
                        .type_(CKBPack::pack(&Some(param.eth_lock_type.clone())))
                        .build(),
                    ETH_ACCOUNT_LOCK_PROGRAM.clone(),
                ),
            );
            CellDep::new_builder().out_point(eth_lock_out_point).build()
        };
        let l2_sudt_dep = {
            let l2_sudt_out_point = random_out_point();
            data_loader.cells.insert(
                l2_sudt_out_point.clone(),
                (
                    CellOutput::new_builder()
                        .capacity(CKBPack::pack(&(ALWAYS_SUCCESS_PROGRAM.len() as u64)))
                        .type_(CKBPack::pack(&Some(param.l2_sudt_type.clone())))
                        .build(),
                    ALWAYS_SUCCESS_PROGRAM.clone(),
                ),
            );
            CellDep::new_builder().out_point(l2_sudt_out_point).build()
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
            l2_sudt_dep,
            eoa_lock_dep,
            eth_lock_dep,
            secp256k1_data_dep,
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
        let hardfork_switch = {
            let switch = GLOBAL_HARDFORK_SWITCH.load();
            HardForkSwitch::new_without_any_enabled()
                .as_builder()
                .rfc_0028(switch.rfc_0028())
                .rfc_0029(switch.rfc_0029())
                .rfc_0030(switch.rfc_0030())
                .rfc_0031(switch.rfc_0031())
                .rfc_0032(switch.rfc_0032())
                .rfc_0036(switch.rfc_0036())
                .rfc_0038(switch.rfc_0038())
                .build()
                .unwrap()
        };
        let consensus = ConsensusBuilder::default()
            .hardfork_switch(hardfork_switch)
            .build();
        let current_epoch_number = GLOBAL_CURRENT_EPOCH_NUMBER.load(Ordering::SeqCst);
        let tx_verify_env = TxVerifyEnv::new_submit(
            &HeaderView::new_advanced_builder()
                .epoch(CKBPack::pack(&current_epoch_number))
                .build(),
        );
        let resolved_tx = build_resolved_tx(&self.inner, &tx);
        let mut verifier =
            TransactionScriptsVerifier::new(&resolved_tx, &consensus, &self.inner, &tx_verify_env);
        verifier.set_debug_printer(|_script, msg| println!("[script debug] {}", msg));
        verifier.verify(MAX_CYCLES)
    }
}

pub fn build_type_id_script(name: &[u8]) -> ckb_types::packed::Script {
    ckb_types::packed::Script::new_builder()
        .code_hash(CKBPack::pack(&ALWAYS_SUCCESS_CODE_HASH.clone()))
        .args(CKBPack::pack(&Bytes::from(name.to_vec())))
        .build()
}

fn random_type_id_script() -> ckb_types::packed::Script {
    let random_bytes: [u8; 32] = rand::random();
    build_type_id_script(&random_bytes)
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

pub fn calculate_state_validator_type_id(input_out_point: ckb_types::packed::OutPoint) -> [u8; 32] {
    let input = ckb_types::packed::CellInput::new_builder()
        .previous_output(input_out_point)
        .build();
    let mut hasher = new_blake2b();
    let output_index: u64 = 0;
    hasher.update(&input.as_bytes());
    hasher.update(&output_index.to_le_bytes());
    let mut expected_type_id = [0u8; 32];
    hasher.finalize(&mut expected_type_id);
    expected_type_id
}
