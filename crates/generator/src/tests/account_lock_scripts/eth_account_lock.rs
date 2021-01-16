use super::*;
use crate::account_lock_manage::{secp256k1::Secp256k1Eth, LockAlgorithm};
use ckb_crypto::secp::{Generator, Privkey, Pubkey};
use ckb_error::assert_error_eq;
use ckb_script::{ScriptError, TransactionScriptsVerifier};
use ckb_types::{
    bytes::Bytes,
    core::{
        cell::{CellMetaBuilder, ResolvedTransaction},
        Capacity, DepType, ScriptHashType, TransactionBuilder, TransactionView,
    },
    packed::{CellDep, CellInput, CellOutput, OutPoint, Script, WitnessArgs},
    prelude::*,
};
use gw_common::builtin_scripts::ETH_ACCOUNT_LOCK;
use gw_types::packed::UnlockAccountWitness;
use rand::{thread_rng, Rng};
use sha3::{Digest, Keccak256};

const ERROR_PUBKEY_BLAKE160_HASH: i8 = -31;

fn gen_tx(dummy: &mut DummyDataLoader, lock_args: Bytes) -> TransactionView {
    let mut rng = thread_rng();
    // setup sighash_all dep
    let script_out_point = {
        let contract_tx_hash = {
            let mut buf = [0u8; 32];
            rng.fill(&mut buf);
            buf.pack()
        };
        OutPoint::new(contract_tx_hash.clone(), 0)
    };
    // dep contract code
    let script_cell = CellOutput::new_builder()
        .capacity(
            Capacity::bytes(ETH_ACCOUNT_LOCK.len())
                .expect("script capacity")
                .pack(),
        )
        .build();
    let script_cell_data_hash = CellOutput::calc_data_hash(&ETH_ACCOUNT_LOCK);
    dummy.cells.insert(
        script_out_point.clone(),
        (script_cell, ETH_ACCOUNT_LOCK.clone()),
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
            Capacity::bytes(SECP256K1_DATA_BIN.len())
                .expect("data capacity")
                .pack(),
        )
        .build();
    dummy.cells.insert(
        secp256k1_data_out_point.clone(),
        (secp256k1_data_cell, SECP256K1_DATA_BIN.clone()),
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
        .output(
            CellOutput::new_builder()
                .capacity(dummy_capacity.pack())
                .build(),
        )
        .output_data(Bytes::new().pack());

    let previous_tx_hash = {
        let mut buf = [0u8; 32];
        rng.fill(&mut buf);
        buf.pack()
    };
    let previous_out_point = OutPoint::new(previous_tx_hash, 0);
    let script = Script::new_builder()
        .args(lock_args.pack())
        .code_hash(script_cell_data_hash.clone())
        .hash_type(ScriptHashType::Data.into())
        .build();
    let previous_output_cell = CellOutput::new_builder()
        .capacity(dummy_capacity.pack())
        .lock(script)
        .build();
    dummy.cells.insert(
        previous_out_point.clone(),
        (previous_output_cell.clone(), Bytes::new()),
    );
    tx_builder
        .input(CellInput::new(previous_out_point, 0))
        .build()
}

fn sign_message(key: &Privkey, message: [u8; 32]) -> gw_types::packed::Signature {
    use gw_types::prelude::*;

    // calculate eth signing message
    let message = {
        let mut hasher = Keccak256::new();
        hasher.update("\x19Ethereum Signed Message:\n32");
        hasher.update(&message);
        let buf = hasher.finalize();
        let mut signing_message = [0u8; 32];
        signing_message.copy_from_slice(&buf[..]);
        ckb_types::H256::from(signing_message)
    };
    let sig = key.sign_recoverable(&message).expect("sign");
    let mut signature = [0u8; 65];
    signature.copy_from_slice(&sig.serialize());
    signature.pack()
}

fn build_resolved_tx(data_loader: &DummyDataLoader, tx: &TransactionView) -> ResolvedTransaction {
    let resolved_cell_deps = tx
        .cell_deps()
        .into_iter()
        .map(|dep| {
            let deps_out_point = dep.clone();
            let (dep_output, dep_data) =
                data_loader.cells.get(&deps_out_point.out_point()).unwrap();
            CellMetaBuilder::from_cell_output(dep_output.to_owned(), dep_data.to_owned())
                .out_point(deps_out_point.out_point().clone())
                .build()
        })
        .collect();

    let mut resolved_inputs = Vec::new();
    for i in 0..tx.inputs().len() {
        let previous_out_point = tx.inputs().get(i).unwrap().previous_output();
        let (input_output, input_data) = data_loader.cells.get(&previous_out_point).unwrap();
        resolved_inputs.push(
            CellMetaBuilder::from_cell_output(input_output.to_owned(), input_data.to_owned())
                .out_point(previous_out_point)
                .build(),
        );
    }

    ResolvedTransaction {
        transaction: tx.clone(),
        resolved_cell_deps,
        resolved_inputs,
        resolved_dep_groups: vec![],
    }
}

pub fn sha3_pubkey_hash(pubkey: &Pubkey) -> Bytes {
    let mut hasher = Keccak256::new();
    hasher.update(&pubkey.as_bytes());
    let buf = hasher.finalize();
    buf[12..].to_vec().into()
}

#[test]
fn test_sign_eth_message() {
    let mut data_loader = DummyDataLoader::new();
    let privkey = Generator::random_privkey();
    let pubkey = privkey.pubkey().expect("pubkey");
    let pubkey_hash = sha3_pubkey_hash(&pubkey);
    let tx = gen_tx(&mut data_loader, pubkey_hash.clone());
    let mut rng = thread_rng();
    let mut message = [0u8; 32];
    rng.fill(&mut message);
    let signature = sign_message(&privkey, message);
    let unlock_witness = {
        let message = gw_types::prelude::Pack::pack(&message);
        UnlockAccountWitness::new_builder()
            .message(message)
            .signature(signature.clone())
            .build()
    };
    let tx = tx
        .as_advanced_builder()
        .set_witnesses(vec![WitnessArgs::new_builder()
            .lock(Some(unlock_witness.as_bytes()).pack())
            .build()
            .as_bytes()
            .pack()])
        .build();
    let resolved_tx = build_resolved_tx(&data_loader, &tx);
    let mut verifier = TransactionScriptsVerifier::new(&resolved_tx, &data_loader);
    verifier.set_debug_printer(|_script, msg| println!("[script debug] {}", msg));
    let verify_result = verifier.verify(MAX_CYCLES);
    verify_result.expect("pass verification");
    let valid = Secp256k1Eth::default()
        .verify_signature(pubkey_hash, signature, message.into())
        .unwrap();
    assert!(valid);
}

#[test]
fn test_wrong_signature() {
    let mut data_loader = DummyDataLoader::new();
    let privkey = Generator::random_privkey();
    let pubkey = privkey.pubkey().expect("pubkey");
    let pubkey_hash = sha3_pubkey_hash(&pubkey);
    let tx = gen_tx(&mut data_loader, pubkey_hash.clone());
    let mut rng = thread_rng();
    let mut message = [0u8; 32];
    rng.fill(&mut message);
    let signature = {
        let mut wrong_message = [0u8; 32];
        rng.fill(&mut wrong_message);
        sign_message(&privkey, wrong_message)
    };
    let unlock_witness = {
        let message = gw_types::prelude::Pack::pack(&message);
        UnlockAccountWitness::new_builder()
            .message(message)
            .signature(signature.clone())
            .build()
    };
    let tx = tx
        .as_advanced_builder()
        .set_witnesses(vec![WitnessArgs::new_builder()
            .lock(Some(unlock_witness.as_bytes()).pack())
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
        ScriptError::ValidationFailure(ERROR_PUBKEY_BLAKE160_HASH)
            .input_lock_script(script_cell_index)
    );
    let valid = Secp256k1Eth::default()
        .verify_signature(pubkey_hash, signature, message.into())
        .unwrap();
    assert!(!valid);
}
