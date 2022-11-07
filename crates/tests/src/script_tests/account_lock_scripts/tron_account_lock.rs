use crate::script_tests::programs::{
    SECP256K1_DATA, TRON_ACCOUNT_LOCK_CODE_HASH, TRON_ACCOUNT_LOCK_PROGRAM,
};
use crate::script_tests::utils::layer1::*;
use crate::testing_tool::chain::{ALWAYS_SUCCESS_CODE_HASH, ALWAYS_SUCCESS_PROGRAM};
use ckb_crypto::secp::{Generator, Privkey, Pubkey};
use ckb_error::assert_error_eq;
use ckb_script::{ScriptError, TransactionScriptsVerifier};
use ckb_types::{
    bytes::Bytes,
    core::{Capacity, DepType, ScriptHashType, TransactionBuilder, TransactionView},
    packed::{CellDep, CellInput, CellOutput, OutPoint, Script, WitnessArgs},
    prelude::*,
};
use gw_types::core::SigningType;
use rand::{thread_rng, Rng};
use sha3::{Digest, Keccak256};

use std::convert::TryInto;

const ERROR_WRONG_SIGNATURE: i8 = 41;

fn gen_tx(
    dummy: &mut DummyDataLoader,
    lock_args: Bytes,
    signing_type: SigningType,
    message: Bytes,
) -> TransactionView {
    let mut rng = thread_rng();
    // setup sighash_all dep
    let script_out_point = {
        let tx_hash = {
            let mut buf = [0u8; 32];
            rng.fill(&mut buf);
            buf.pack()
        };
        OutPoint::new(tx_hash, 0)
    };
    let owner_lock_script_out_point = {
        let tx_hash = {
            let mut buf = [0u8; 32];
            rng.fill(&mut buf);
            buf.pack()
        };
        OutPoint::new(tx_hash, 0)
    };
    // dep contract code
    // eth account lock
    let script_cell = CellOutput::new_builder()
        .capacity(
            Capacity::bytes(TRON_ACCOUNT_LOCK_PROGRAM.len())
                .expect("script capacity")
                .pack(),
        )
        .build();
    let script_cell_data_hash = CellOutput::calc_data_hash(&TRON_ACCOUNT_LOCK_PROGRAM);
    dummy.cells.insert(
        script_out_point.clone(),
        (script_cell, TRON_ACCOUNT_LOCK_PROGRAM.clone()),
    );
    // owner lock
    let script_cell = CellOutput::new_builder()
        .capacity(
            Capacity::bytes(ALWAYS_SUCCESS_PROGRAM.len())
                .expect("script capacity")
                .pack(),
        )
        .build();
    dummy.cells.insert(
        owner_lock_script_out_point.clone(),
        (script_cell, ALWAYS_SUCCESS_PROGRAM.clone()),
    );
    // owner lock cell
    let owner_lock_cell = CellOutput::new_builder()
        .lock(
            Script::new_builder()
                .code_hash((*ALWAYS_SUCCESS_CODE_HASH).pack())
                .hash_type(ScriptHashType::Data.into())
                .build(),
        )
        .build();
    let owner_lock_hash = owner_lock_cell.lock().calc_script_hash().unpack();
    let owner_lock_cell_out_point = {
        let tx_hash = {
            let mut buf = [0u8; 32];
            rng.fill(&mut buf);
            buf.pack()
        };
        OutPoint::new(tx_hash, 0)
    };
    dummy.cells.insert(
        owner_lock_cell_out_point.clone(),
        (owner_lock_cell, Bytes::default()),
    );
    // setup secp256k1_data dep
    let secp256k1_data_out_point = {
        let tx_hash = {
            let mut buf = [0u8; 32];
            rng.fill(&mut buf);
            buf.pack()
        };
        OutPoint::new(tx_hash, 0)
    };
    let secp256k1_data_cell = CellOutput::new_builder()
        .capacity(
            Capacity::bytes(SECP256K1_DATA.len())
                .expect("data capacity")
                .pack(),
        )
        .build();
    dummy.cells.insert(
        secp256k1_data_out_point.clone(),
        (secp256k1_data_cell, SECP256K1_DATA.clone()),
    );
    // setup default tx builder
    let dummy_capacity = Capacity::shannons(42);
    let tx_builder = TransactionBuilder::default()
        .cell_dep(
            CellDep::new_builder()
                .out_point(script_out_point)
                .dep_type(DepType::Code.into())
                .build(),
        )
        .cell_dep(
            CellDep::new_builder()
                .out_point(secp256k1_data_out_point)
                .dep_type(DepType::Code.into())
                .build(),
        )
        .cell_dep(
            CellDep::new_builder()
                .out_point(owner_lock_script_out_point)
                .dep_type(DepType::Code.into())
                .build(),
        )
        .output(
            CellOutput::new_builder()
                .capacity(dummy_capacity.pack())
                .build(),
        )
        .output_data(Bytes::new().pack());

    let previous_out_point = {
        let previous_tx_hash = {
            let mut buf = [0u8; 32];
            rng.fill(&mut buf);
            buf.pack()
        };
        OutPoint::new(previous_tx_hash, 0)
    };
    let previous_output_cell = {
        let script = Script::new_builder()
            .args(lock_args.pack())
            .code_hash(script_cell_data_hash)
            .hash_type(ScriptHashType::Data.into())
            .build();
        CellOutput::new_builder()
            .capacity(dummy_capacity.pack())
            .lock(script)
            .build()
    };
    let mut input_data = owner_lock_hash.as_bytes().to_vec();
    input_data.push(signing_type.into());
    input_data.extend_from_slice(&message);
    dummy.cells.insert(
        previous_out_point.clone(),
        (previous_output_cell, input_data.into()),
    );
    tx_builder
        .input(CellInput::new(previous_out_point, 0))
        .input(CellInput::new(owner_lock_cell_out_point, 0))
        .build()
}

fn sign_message(key: &Privkey, message: [u8; 32]) -> Bytes {
    // calculate eth signing message
    let message = {
        let mut hasher = Keccak256::new();
        hasher.update("\x19TRON Signed Message:\n32");
        hasher.update(&message);
        let buf = hasher.finalize();
        let mut signing_message = [0u8; 32];
        signing_message.copy_from_slice(&buf[..]);
        ckb_types::H256::from(signing_message)
    };
    let sig = key.sign_recoverable(&message).expect("sign");
    let mut signature = [0u8; 65];
    signature.copy_from_slice(&sig.serialize());
    if signature[64] == 1 {
        signature[64] = 28;
    }
    signature.to_vec().into()
}

pub fn sha3_pubkey_hash(pubkey: &Pubkey) -> Bytes {
    let mut hasher = Keccak256::new();
    hasher.update(&pubkey.as_bytes());
    let buf = hasher.finalize();
    buf[12..].to_vec().into()
}

#[test]
fn test_sign_tron_message() {
    let mut data_loader = DummyDataLoader::default();
    let privkey = Generator::random_privkey();
    let pubkey = privkey.pubkey().expect("pubkey");
    let pubkey_hash = sha3_pubkey_hash(&pubkey);
    let mut rng = thread_rng();
    let mut message = [0u8; 32];
    rng.fill(&mut message);
    let signature = sign_message(&privkey, message);
    let lock_args = {
        let rollup_script_hash = [42u8; 32];
        let mut args = rollup_script_hash.to_vec();
        args.extend_from_slice(&pubkey_hash);
        args.into()
    };
    let tx = gen_tx(
        &mut data_loader,
        lock_args,
        SigningType::WithPrefix,
        message.to_vec().into(),
    );
    let tx = tx
        .as_advanced_builder()
        .set_witnesses(vec![WitnessArgs::new_builder()
            .lock(Some(signature).pack())
            .build()
            .as_bytes()
            .pack()])
        .build();
    let resolved_tx = build_resolved_tx(&data_loader, &tx);
    let mut verifier = TransactionScriptsVerifier::new(&resolved_tx, &data_loader);
    verifier.set_debug_printer(|_script, msg| println!("[script debug] {}", msg));
    let verify_result = verifier.verify(MAX_CYCLES);
    verify_result.expect("pass verification");
}

#[test]
fn test_submit_signing_tron_message() {
    let mut data_loader = DummyDataLoader::default();
    let privkey = Generator::random_privkey();
    let pubkey = privkey.pubkey().expect("pubkey");
    let pubkey_hash = sha3_pubkey_hash(&pubkey);
    let mut rng = thread_rng();
    let mut message = [0u8; 32];
    rng.fill(&mut message);
    let signature = sign_message(&privkey, message);
    let lock_args = {
        let rollup_script_hash = [42u8; 32];
        let mut args = rollup_script_hash.to_vec();
        args.extend_from_slice(&pubkey_hash);
        args.into()
    };
    let signing_message: [u8; 32] = {
        let mut hasher = Keccak256::new();
        hasher.update("\x19TRON Signed Message:\n32");
        hasher.update(&message);
        let buf = hasher.finalize();
        buf.to_vec().try_into().unwrap()
    };
    let tx = gen_tx(
        &mut data_loader,
        lock_args,
        SigningType::Raw,
        signing_message.to_vec().into(),
    );
    let tx = tx
        .as_advanced_builder()
        .set_witnesses(vec![WitnessArgs::new_builder()
            .lock(Some(signature).pack())
            .build()
            .as_bytes()
            .pack()])
        .build();
    let resolved_tx = build_resolved_tx(&data_loader, &tx);
    let mut verifier = TransactionScriptsVerifier::new(&resolved_tx, &data_loader);
    verifier.set_debug_printer(|_script, msg| println!("[script debug] {}", msg));
    let verify_result = verifier.verify(MAX_CYCLES);
    verify_result.expect("pass verification");
}

#[test]
fn test_wrong_signature() {
    let mut data_loader = DummyDataLoader::default();
    let privkey = Generator::random_privkey();
    let pubkey = privkey.pubkey().expect("pubkey");
    let pubkey_hash = sha3_pubkey_hash(&pubkey);
    let lock_args = {
        let rollup_script_hash = [42u8; 32];
        let mut args = rollup_script_hash.to_vec();
        args.extend_from_slice(&pubkey_hash);
        args.into()
    };
    let mut rng = thread_rng();
    let mut message = [0u8; 32];
    rng.fill(&mut message);
    let signature = {
        let mut wrong_message = [0u8; 32];
        rng.fill(&mut wrong_message);
        sign_message(&privkey, wrong_message)
    };
    let tx = gen_tx(
        &mut data_loader,
        lock_args,
        SigningType::Raw,
        message.to_vec().into(),
    );
    let tx = tx
        .as_advanced_builder()
        .set_witnesses(vec![WitnessArgs::new_builder()
            .lock(Some(signature).pack())
            .build()
            .as_bytes()
            .pack()])
        .build();
    let resolved_tx = build_resolved_tx(&data_loader, &tx);
    let mut verifier = TransactionScriptsVerifier::new(&resolved_tx, &data_loader);
    verifier.set_debug_printer(|_script, msg| println!("[script debug] {}", msg));
    let verify_result = verifier.verify(MAX_CYCLES);
    let script_cell_index = 0;
    assert_error_eq!(
        verify_result.unwrap_err(),
        ScriptError::ValidationFailure(
            format!(
                "by-data-hash/{}",
                ckb_types::H256(*TRON_ACCOUNT_LOCK_CODE_HASH)
            ),
            ERROR_WRONG_SIGNATURE
        )
        .input_lock_script(script_cell_index)
    );
}
