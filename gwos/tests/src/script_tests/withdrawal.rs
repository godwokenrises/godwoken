use super::utils::init_env_log;
use super::utils::layer1::build_simple_tx_with_out_point;
use super::utils::rollup::{build_rollup_locked_cell, CellContext};

use crate::testing_tool::programs::{
    ALWAYS_SUCCESS_CODE_HASH, ALWAYS_SUCCESS_PROGRAM, ANYONE_CAN_PAY_LOCK_PROGRAM, SECP256K1_DATA,
    WITHDRAWAL_LOCK_PROGRAM,
};

use ckb_error::assert_error_eq;
use ckb_script::ScriptError;
use ckb_types::core::TransactionView;
use ckb_types::prelude::{Builder, Entity};
use gw_common::blake2b::new_blake2b;
use gw_types::bytes::Bytes;
use gw_types::core::ScriptHashType;
use gw_types::packed::{
    CellDep, CellInput, CellOutput, GlobalState, OutPoint, RollupConfig, Script,
    UnlockWithdrawalViaFinalize, UnlockWithdrawalWitness, UnlockWithdrawalWitnessUnion,
    WithdrawalLockArgs, WitnessArgs,
};
use gw_types::prelude::Pack;
use secp256k1::rand::rngs::OsRng;
use secp256k1::{Message, Secp256k1, SecretKey};

const OWNER_CELL_NOT_FOUND_ERROR: i8 = 8;

#[test]
fn test_unlock_withdrawal_via_finalize_by_input_owner_cell() {
    init_env_log();

    const DEFAULT_CAPACITY: u64 = 1000 * 10u64.pow(8);

    let rollup_type_script = random_always_success_script();
    let rollup_type_hash = rollup_type_script.hash();
    let (mut verify_ctx, script_ctx) = build_verify_context();

    let last_finalized_block_number = rand::random::<u64>() + 100;
    let rollup_cell = {
        let global_state = GlobalState::new_builder()
            .last_finalized_block_number(last_finalized_block_number.pack())
            .build();

        let output = CellOutput::new_builder()
            .lock(random_always_success_script())
            .type_(Some(rollup_type_script).pack())
            .capacity(DEFAULT_CAPACITY.pack())
            .build();

        (output, global_state.as_bytes())
    };
    let rollup_dep = {
        let out_point = verify_ctx.insert_cell(rollup_cell.0.to_ckb(), rollup_cell.1);
        CellDep::new_builder().out_point(out_point.to_gw()).build()
    };

    let (sk, pk) = {
        let secp = Secp256k1::new();
        let mut rng = OsRng::new().unwrap();
        secp.generate_keypair(&mut rng)
    };
    let (err_sk, _err_pk) = {
        let secp = Secp256k1::new();
        let mut rng = OsRng::new().unwrap();
        secp.generate_keypair(&mut rng)
    };
    let owner_lock = {
        let args = {
            let mut buf = [0u8; 32];
            let mut hasher = new_blake2b();
            hasher.update(&pk.serialize());
            hasher.finalize(&mut buf);

            Bytes::copy_from_slice(&buf[..20])
        };

        Script::new_builder()
            .code_hash(script_ctx.acp.script.hash().pack())
            .hash_type(ScriptHashType::Type.into())
            .args(args.pack())
            .build()
    };
    let finalized_withdrawal_cell = {
        let lock_args = WithdrawalLockArgs::new_builder()
            .account_script_hash(random_always_success_script().hash().pack())
            .withdrawal_block_hash(random_always_success_script().hash().pack())
            .withdrawal_block_number(last_finalized_block_number.saturating_sub(1).pack())
            .owner_lock_hash(owner_lock.hash().pack())
            .build();
        let mut args = Vec::new();
        args.extend_from_slice(&lock_args.as_bytes());
        args.extend_from_slice(&(owner_lock.as_bytes().len() as u32).to_be_bytes());
        args.extend_from_slice(&owner_lock.as_bytes());

        let output = build_rollup_locked_cell(
            &rollup_type_hash,
            &script_ctx.withdrawal.script.hash(),
            DEFAULT_CAPACITY,
            args.into(),
        );

        (output, 0u128.pack().as_bytes())
    };
    let finalized_withdrawal_input = {
        let out_point =
            verify_ctx.insert_cell(finalized_withdrawal_cell.0.clone(), 0u128.pack().as_bytes());
        CellInput::new_builder()
            .previous_output(out_point.to_gw())
            .build()
    };
    let owner_input = {
        let output = CellOutput::new_builder()
            .capacity(DEFAULT_CAPACITY.pack())
            .lock(owner_lock.clone())
            .build();

        let out_point = verify_ctx.insert_cell(output.to_ckb(), 0u128.pack().as_bytes());
        CellInput::new_builder()
            .previous_output(out_point.to_gw())
            .build()
    };
    let output_cell = {
        let output = CellOutput::new_builder()
            .capacity((DEFAULT_CAPACITY * 2).pack())
            .lock(owner_lock)
            .build();

        (output.to_ckb(), 0u128.pack().as_bytes())
    };
    let unlock_via_finalize_witness = {
        let unlock_args = UnlockWithdrawalViaFinalize::new_builder().build();
        let unlock_witness = UnlockWithdrawalWitness::new_builder()
            .set(UnlockWithdrawalWitnessUnion::UnlockWithdrawalViaFinalize(
                unlock_args,
            ))
            .build();
        WitnessArgs::new_builder()
            .lock(Some(unlock_witness.as_bytes()).pack())
            .build()
    };

    let tx = build_simple_tx_with_out_point(
        &mut verify_ctx.inner,
        finalized_withdrawal_cell,
        finalized_withdrawal_input.to_ckb().previous_output(),
        output_cell,
    )
    .as_advanced_builder()
    .witness(unlock_via_finalize_witness.as_bytes().to_ckb())
    .input(owner_input.to_ckb())
    .witness(Default::default())
    .cell_dep(script_ctx.withdrawal.dep.to_ckb())
    .cell_dep(script_ctx.acp.dep.to_ckb())
    .cell_dep(script_ctx.secp256k1_data.dep.to_ckb())
    .cell_dep(rollup_dep.to_ckb())
    .build();

    let err_sign_tx = sign_tx(tx.clone(), 1, &err_sk);
    verify_ctx
        .verify_tx(err_sign_tx)
        .expect_err("wrong privtate key");

    let sign_tx = sign_tx(tx, 1, &sk);
    verify_ctx.verify_tx(sign_tx).expect("success");
}

#[test]
fn test_unlock_withdrawal_via_finalize_by_switch_indexed_output_to_owner_lock() {
    init_env_log();

    const DEFAULT_CAPACITY: u64 = 1000 * 10u64.pow(8);

    let rollup_type_script = random_always_success_script();
    let rollup_type_hash = rollup_type_script.hash();
    let (mut verify_ctx, script_ctx) = build_verify_context();

    let last_finalized_block_number = rand::random::<u64>() + 100;
    let rollup_cell = {
        let global_state = GlobalState::new_builder()
            .last_finalized_block_number(last_finalized_block_number.pack())
            .build();

        let output = CellOutput::new_builder()
            .lock(random_always_success_script())
            .type_(Some(rollup_type_script).pack())
            .capacity(DEFAULT_CAPACITY.pack())
            .build();

        (output, global_state.as_bytes())
    };
    let rollup_dep = {
        let out_point = verify_ctx.insert_cell(rollup_cell.0.to_ckb(), rollup_cell.1);
        CellDep::new_builder().out_point(out_point.to_gw()).build()
    };

    let owner_lock = random_always_success_script();
    let finalized_withdrawal_cell = {
        let lock_args = WithdrawalLockArgs::new_builder()
            .account_script_hash(random_always_success_script().hash().pack())
            .withdrawal_block_hash(random_always_success_script().hash().pack())
            .withdrawal_block_number(last_finalized_block_number.saturating_sub(1).pack())
            .owner_lock_hash(owner_lock.hash().pack())
            .build();

        let mut args = Vec::new();
        args.extend_from_slice(&lock_args.as_bytes());
        args.extend_from_slice(&(owner_lock.as_bytes().len() as u32).to_be_bytes());
        args.extend_from_slice(&owner_lock.as_bytes());

        let output = build_rollup_locked_cell(
            &rollup_type_hash,
            &script_ctx.withdrawal.script.hash(),
            DEFAULT_CAPACITY,
            Bytes::from(args),
        );

        (output, 0u128.pack().as_bytes())
    };
    let finalized_withdrawal_input_1 = {
        let out_point =
            verify_ctx.insert_cell(finalized_withdrawal_cell.0.clone(), 0u128.pack().as_bytes());
        CellInput::new_builder()
            .previous_output(out_point.to_gw())
            .build()
    };
    let finalized_withdrawal_input_2 = {
        let out_point =
            verify_ctx.insert_cell(finalized_withdrawal_cell.0.clone(), 0u128.pack().as_bytes());
        CellInput::new_builder()
            .previous_output(out_point.to_gw())
            .build()
    };
    let output_cell = {
        let output = CellOutput::new_builder()
            .capacity(DEFAULT_CAPACITY.pack())
            .lock(owner_lock)
            .build();

        (output.to_ckb(), 0u128.pack().as_bytes())
    };
    let unlock_via_finalize_witness = {
        let unlock_args = UnlockWithdrawalViaFinalize::new_builder().build();
        let unlock_witness = UnlockWithdrawalWitness::new_builder()
            .set(UnlockWithdrawalWitnessUnion::UnlockWithdrawalViaFinalize(
                unlock_args,
            ))
            .build();
        WitnessArgs::new_builder()
            .lock(Some(unlock_witness.as_bytes()).pack())
            .build()
    };

    // Try single withdrawal
    let tx = build_simple_tx_with_out_point(
        &mut verify_ctx.inner,
        finalized_withdrawal_cell,
        finalized_withdrawal_input_1.to_ckb().previous_output(),
        output_cell.clone(),
    )
    .as_advanced_builder()
    .witness(unlock_via_finalize_witness.as_bytes().to_ckb())
    .cell_dep(script_ctx.withdrawal.dep.to_ckb())
    .cell_dep(rollup_dep.to_ckb())
    .build();

    verify_ctx.verify_tx(tx.clone()).expect("success");

    // Try multiple withdrawals without indexed output
    let tx = tx
        .as_advanced_builder()
        .input(finalized_withdrawal_input_2.to_ckb())
        .witness(Default::default())
        .build();

    let err = verify_ctx.verify_tx(tx.clone()).unwrap_err();
    let expected_err = ScriptError::ValidationFailure(
        format!(
            "by-type-hash/{}",
            ckb_types::H256(script_ctx.withdrawal.script.hash())
        ),
        OWNER_CELL_NOT_FOUND_ERROR,
    )
    .input_lock_script(0);
    assert_error_eq!(err, expected_err);

    // Fill incorrect output
    let err_tx = tx
        .as_advanced_builder()
        .output(output_cell.0.clone())
        .output_data(1u128.pack().as_bytes().to_ckb()) // ERROR: change output data
        .build();

    let err = verify_ctx.verify_tx(err_tx).unwrap_err();
    let expected_err = ScriptError::ValidationFailure(
        format!(
            "by-type-hash/{}",
            ckb_types::H256(script_ctx.withdrawal.script.hash())
        ),
        OWNER_CELL_NOT_FOUND_ERROR,
    )
    .input_lock_script(0);
    assert_error_eq!(err, expected_err);

    // Fill incorrect output lock
    let err_output = CellOutput::new_builder()
        .capacity(DEFAULT_CAPACITY.pack())
        .lock(random_always_success_script()) // ERROR: dirrerent output lock
        .build();
    let err_tx = tx
        .as_advanced_builder()
        .output(err_output.to_ckb())
        .output_data(output_cell.1.to_ckb())
        .build();

    let err = verify_ctx.verify_tx(err_tx).unwrap_err();
    let expected_err = ScriptError::ValidationFailure(
        format!(
            "by-type-hash/{}",
            ckb_types::H256(script_ctx.withdrawal.script.hash())
        ),
        OWNER_CELL_NOT_FOUND_ERROR,
    )
    .input_lock_script(0);
    assert_error_eq!(err, expected_err);

    // Fill correct output
    let tx = tx
        .as_advanced_builder()
        .output(output_cell.0)
        .output_data(output_cell.1.to_ckb())
        .build();

    verify_ctx.verify_tx(tx).expect("success");
}

#[test]
fn test_unlock_withdrawal_via_finalize_fallback_to_input_owner_cell() {
    init_env_log();

    const DEFAULT_CAPACITY: u64 = 1000 * 10u64.pow(8);

    let rollup_type_script = random_always_success_script();
    let rollup_type_hash = rollup_type_script.hash();
    let (mut verify_ctx, script_ctx) = build_verify_context();

    let last_finalized_block_number = rand::random::<u64>() + 100;
    let rollup_cell = {
        let global_state = GlobalState::new_builder()
            .last_finalized_block_number(last_finalized_block_number.pack())
            .build();

        let output = CellOutput::new_builder()
            .lock(random_always_success_script())
            .type_(Some(rollup_type_script).pack())
            .capacity(DEFAULT_CAPACITY.pack())
            .build();

        (output, global_state.as_bytes())
    };
    let rollup_dep = {
        let out_point = verify_ctx.insert_cell(rollup_cell.0.to_ckb(), rollup_cell.1);
        CellDep::new_builder().out_point(out_point.to_gw()).build()
    };

    let (sk, pk) = {
        let secp = Secp256k1::new();
        let mut rng = OsRng::new().unwrap();
        secp.generate_keypair(&mut rng)
    };
    let owner_lock = {
        let args = {
            let mut buf = [0u8; 32];
            let mut hasher = new_blake2b();
            hasher.update(&pk.serialize());
            hasher.finalize(&mut buf);

            Bytes::copy_from_slice(&buf[..20])
        };

        Script::new_builder()
            .code_hash(script_ctx.acp.script.hash().pack())
            .hash_type(ScriptHashType::Type.into())
            .args(args.pack())
            .build()
    };
    let finalized_withdrawal_cell = {
        let lock_args = WithdrawalLockArgs::new_builder()
            .account_script_hash(random_always_success_script().hash().pack())
            .withdrawal_block_hash(random_always_success_script().hash().pack())
            .withdrawal_block_number(last_finalized_block_number.saturating_sub(1).pack())
            .owner_lock_hash(owner_lock.hash().pack())
            .build();

        let mut args = Vec::new();
        args.extend_from_slice(&lock_args.as_bytes());
        args.extend_from_slice(&(owner_lock.as_bytes().len() as u32).to_be_bytes());
        args.extend_from_slice(&owner_lock.as_bytes());

        let output = build_rollup_locked_cell(
            &rollup_type_hash,
            &script_ctx.withdrawal.script.hash(),
            DEFAULT_CAPACITY,
            args.into(),
        );

        (output, 0u128.pack().as_bytes())
    };
    let finalized_withdrawal_input = {
        let out_point =
            verify_ctx.insert_cell(finalized_withdrawal_cell.0.clone(), 0u128.pack().as_bytes());
        CellInput::new_builder()
            .previous_output(out_point.to_gw())
            .build()
    };
    let owner_input = {
        let output = CellOutput::new_builder()
            .capacity(DEFAULT_CAPACITY.pack())
            .lock(owner_lock.clone())
            .build();

        let out_point = verify_ctx.insert_cell(output.to_ckb(), 0u128.pack().as_bytes());
        CellInput::new_builder()
            .previous_output(out_point.to_gw())
            .build()
    };
    // ERROR: wrong output capacity, can only unlock by input owner cell
    let output_cell = {
        let output = CellOutput::new_builder()
            .capacity((DEFAULT_CAPACITY - 2).pack())
            .lock(owner_lock)
            .build();

        (output.to_ckb(), 0u128.pack().as_bytes())
    };
    let unlock_via_finalize_witness = {
        let unlock_args = UnlockWithdrawalViaFinalize::new_builder().build();
        let unlock_witness = UnlockWithdrawalWitness::new_builder()
            .set(UnlockWithdrawalWitnessUnion::UnlockWithdrawalViaFinalize(
                unlock_args,
            ))
            .build();
        WitnessArgs::new_builder()
            .lock(Some(unlock_witness.as_bytes()).pack())
            .build()
    };

    // Try unlock directly, expect failure (see ERROR above)
    let tx = build_simple_tx_with_out_point(
        &mut verify_ctx.inner,
        finalized_withdrawal_cell,
        finalized_withdrawal_input.to_ckb().previous_output(),
        output_cell,
    )
    .as_advanced_builder()
    .witness(unlock_via_finalize_witness.as_bytes().to_ckb())
    .cell_dep(script_ctx.withdrawal.dep.to_ckb())
    .cell_dep(rollup_dep.to_ckb())
    .build();

    let err = verify_ctx.verify_tx(tx.clone()).unwrap_err();
    let expected_err = ScriptError::ValidationFailure(
        format!(
            "by-type-hash/{}",
            ckb_types::H256(script_ctx.withdrawal.script.hash())
        ),
        OWNER_CELL_NOT_FOUND_ERROR,
    )
    .input_lock_script(0);
    assert_error_eq!(err, expected_err);

    // Try input owner cell
    let tx = tx
        .as_advanced_builder()
        .input(owner_input.to_ckb())
        .witness(Default::default())
        .cell_dep(script_ctx.acp.dep.to_ckb())
        .cell_dep(script_ctx.secp256k1_data.dep.to_ckb())
        .cell_dep(rollup_dep.to_ckb())
        .build();

    let sign_tx = sign_tx(tx, 1, &sk);
    verify_ctx.verify_tx(sign_tx).expect("success");
}

struct ScriptDep {
    script: Script,
    dep: CellDep,
}

struct ScriptContext {
    withdrawal: ScriptDep,
    _sudt: ScriptDep,
    acp: ScriptDep,
    secp256k1_data: ScriptDep,
}

fn build_verify_context() -> (CellContext, ScriptContext) {
    let withdrawal_lock_type = random_always_success_script();
    let sudt_type = random_always_success_script();
    let acp_lock_type = random_always_success_script();
    let secp256k1_data_type = random_always_success_script();

    let config = RollupConfig::new_builder()
        .withdrawal_script_type_hash(withdrawal_lock_type.hash().pack())
        .l1_sudt_script_type_hash(sudt_type.hash().pack())
        .finality_blocks(10u64.pack())
        .build();
    let mut ctx = CellContext::new(&config, Default::default());

    let withdrawal_output = CellOutput::new_builder()
        .lock(random_always_success_script())
        .type_(Some(withdrawal_lock_type.clone()).pack())
        .build();
    let withdrawal_cell_dep = {
        let out_point =
            ctx.insert_cell(withdrawal_output.to_ckb(), WITHDRAWAL_LOCK_PROGRAM.clone());
        CellDep::new_builder().out_point(out_point.to_gw()).build()
    };
    ctx.withdrawal_lock_dep = withdrawal_cell_dep.to_ckb();

    let sudt_output = CellOutput::new_builder()
        .lock(random_always_success_script())
        .type_(Some(sudt_type.clone()).pack())
        .build();
    let sudt_cell_dep = {
        let out_point = ctx.insert_cell(sudt_output.to_ckb(), ALWAYS_SUCCESS_PROGRAM.clone());
        CellDep::new_builder().out_point(out_point.to_gw()).build()
    };

    let acp_output = CellOutput::new_builder()
        .lock(random_always_success_script())
        .type_(Some(acp_lock_type.clone()).pack())
        .build();
    let acp_cell_dep = {
        let out_point = ctx.insert_cell(acp_output.to_ckb(), ANYONE_CAN_PAY_LOCK_PROGRAM.clone());
        CellDep::new_builder().out_point(out_point.to_gw()).build()
    };

    let secp256k1_data_output = CellOutput::new_builder()
        .lock(random_always_success_script())
        .type_(Some(secp256k1_data_type.clone()).pack())
        .build();
    let secp256k1_data_dep = {
        let out_point = ctx.insert_cell(secp256k1_data_output.to_ckb(), SECP256K1_DATA.clone());
        CellDep::new_builder().out_point(out_point.to_gw()).build()
    };

    let script_ctx = ScriptContext {
        withdrawal: ScriptDep {
            script: withdrawal_lock_type,
            dep: withdrawal_cell_dep,
        },
        _sudt: ScriptDep {
            script: sudt_type,
            dep: sudt_cell_dep,
        },
        acp: ScriptDep {
            script: acp_lock_type,
            dep: acp_cell_dep,
        },
        secp256k1_data: ScriptDep {
            script: secp256k1_data_type,
            dep: secp256k1_data_dep,
        },
    };

    (ctx, script_ctx)
}

fn random_always_success_script() -> Script {
    let random_bytes: [u8; 32] = rand::random();
    Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.clone().pack())
        .args(Bytes::from(random_bytes.to_vec()).pack())
        .build()
}

fn sign_tx(tx: TransactionView, witness_idx: usize, sk: &SecretKey) -> TransactionView {
    const SIGNATURE_SIZE: usize = 65;

    // Digest witness
    let zero_lock = Bytes::copy_from_slice(&[0u8; SIGNATURE_SIZE]);
    let witness_for_digest = WitnessArgs::new_builder()
        .lock(Some(zero_lock).pack())
        .build();

    let tx_hash = tx.hash();
    let witness_len = witness_for_digest.as_bytes().len() as u64;
    let mut blake2b = new_blake2b();
    let mut message = [0u8; 32];

    blake2b.update(&tx_hash.raw_data());
    blake2b.update(&witness_len.to_le_bytes());
    blake2b.update(&witness_for_digest.as_bytes());
    blake2b.finalize(&mut message);

    let secp = Secp256k1::new();
    let message = Message::from_slice(&message).unwrap();
    let sig = {
        let sig = secp.sign_recoverable(&message, sk);
        let (rec_id, bytes) = sig.serialize_compact();
        assert!(rec_id.to_i32() >= 0 && rec_id.to_i32() < 4);

        let mut buf = [0u8; 65];
        buf[..64].copy_from_slice(&bytes[..64]);
        buf[64] = rec_id.to_i32() as u8;
        Bytes::copy_from_slice(&buf[..65])
    };

    let mut signed_witnesses: Vec<_> = tx.witnesses().into_iter().collect();
    let witness = WitnessArgs::new_builder().lock(Some(sig).pack()).build();
    *signed_witnesses.get_mut(witness_idx).unwrap() = witness.as_bytes().to_ckb();

    tx.as_advanced_builder()
        .set_witnesses(signed_witnesses)
        .build()
}

mod conversion {
    use ckb_types::packed::{Bytes, CellDep, CellInput, CellOutput, OutPoint, Script, WitnessArgs};
    use ckb_types::prelude::{Entity, Pack};

    pub trait ToCKBType<T> {
        fn to_ckb(&self) -> T;
    }

    macro_rules! impl_to_ckb {
        ($type_:tt) => {
            impl ToCKBType<$type_> for super::$type_ {
                fn to_ckb(&self) -> $type_ {
                    $type_::new_unchecked(self.as_bytes())
                }
            }
        };
    }
    impl_to_ckb!(Script);
    impl_to_ckb!(CellInput);
    impl_to_ckb!(CellOutput);
    impl_to_ckb!(WitnessArgs);
    impl_to_ckb!(CellDep);

    impl ToCKBType<Bytes> for super::Bytes {
        fn to_ckb(&self) -> Bytes {
            self.pack()
        }
    }

    pub trait ToGWType<T> {
        fn to_gw(&self) -> T;
    }

    macro_rules! impl_to_gw {
        ($type_:tt) => {
            impl ToGWType<super::$type_> for $type_ {
                fn to_gw(&self) -> super::$type_ {
                    super::$type_::new_unchecked(self.as_bytes())
                }
            }
        };
    }

    impl_to_gw!(OutPoint);
    impl_to_gw!(CellOutput);
    impl_to_gw!(Script);
}

use conversion::{ToCKBType, ToGWType};
