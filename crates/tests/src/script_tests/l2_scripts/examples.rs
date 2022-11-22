use crate::script_tests::l2_scripts::SUDT_TOTAL_SUPPLY_PROGRAM_PATH;
use crate::testing_tool::chain::ALWAYS_SUCCESS_CODE_HASH;
use std::sync::Arc;

use super::{
    new_block_info, DummyChainStore, SudtLog, SudtLogType, ACCOUNT_OP_PROGRAM_CODE_HASH,
    ACCOUNT_OP_PROGRAM_PATH, GW_LOG_SUDT_TRANSFER, RECOVER_PROGRAM_CODE_HASH, RECOVER_PROGRAM_PATH,
    SUDT_TOTAL_SUPPLY_PROGRAM_CODE_HASH, SUM_PROGRAM_CODE_HASH, SUM_PROGRAM_PATH,
};
use gw_common::smt::SMT;
use gw_common::{
    builtins::ETH_REGISTRY_ACCOUNT_ID, h256_ext::H256Ext, registry_address::RegistryAddress,
    state::State, H256,
};
use gw_config::{BackendConfig, BackendForkConfig, BackendType};
use gw_generator::backend_manage::BackendManage;
use gw_generator::{
    account_lock_manage::{
        always_success::AlwaysSuccess, secp256k1::Secp256k1Eth, AccountLockManage,
    },
    error::TransactionError,
    syscalls::error_codes::{GW_ERROR_ACCOUNT_NOT_FOUND, GW_ERROR_RECOVER, GW_FATAL_UNKNOWN_ARGS},
    traits::StateExt,
    Generator,
};
use gw_store::smt::smt_store::SMTStateStore;
use gw_store::snapshot::StoreSnapshot;
use gw_store::state::overlay::mem_state::MemStateTree;
use gw_store::state::overlay::mem_store::MemStore;
use gw_store::state::traits::JournalDB;
use gw_store::state::MemStateDB;
use gw_store::Store;
use gw_types::core::AllowedContractType;
use gw_types::packed::AllowedTypeHash;
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{RawL2Transaction, RollupConfig, Script},
    prelude::*,
    U256,
};
use gw_utils::RollupContext;

fn new_state(store: StoreSnapshot) -> MemStateDB {
    let smt = SMT::new(H256::zero(), SMTStateStore::new(MemStore::new(store)));
    let inner = MemStateTree::new(smt, 0);
    MemStateDB::new(inner)
}

#[test]
fn test_example_sum() {
    let store = Store::open_tmp().unwrap();
    let mut tree = new_state(store.get_snapshot());
    let chain_view = DummyChainStore;
    let sender_script = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.pack())
        .args([1u8; 20].to_vec().pack())
        .hash_type(ScriptHashType::Type.into())
        .build();
    let from_id = tree
        .create_account_from_script(sender_script.clone())
        .expect("create account");
    tree.mapping_registry_address_to_script_hash(
        RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, vec![42u8; 20]),
        sender_script.hash().into(),
    )
    .unwrap();
    let init_value: u64 = 0;

    let contract_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash(SUM_PROGRAM_CODE_HASH.pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");

    // run handle message
    {
        // NOTICE in this test we won't need SUM validator
        let backend_manage = BackendManage::from_config(vec![BackendForkConfig {
            fork_height: 0,
            backends: vec![BackendConfig {
                validator_path: SUM_PROGRAM_PATH.to_path_buf(),
                generator_path: SUM_PROGRAM_PATH.to_path_buf(),
                validator_script_type_hash: (*SUM_PROGRAM_CODE_HASH).into(),
                backend_type: BackendType::Unknown,
            }],
        }])
        .unwrap();
        let mut account_lock_manage = AccountLockManage::default();
        account_lock_manage
            .register_lock_algorithm(H256::zero(), Arc::new(AlwaysSuccess::default()));
        let rollup_context = RollupContext {
            rollup_config: Default::default(),
            rollup_script_hash: [42u8; 32].into(),
            ..Default::default()
        };
        let generator = Generator::new(
            backend_manage,
            account_lock_manage,
            rollup_context,
            Default::default(),
        );
        let mut sum_value = init_value;
        for (number, add_value) in &[(1u64, 7u64), (2u64, 16u64)] {
            let block_info = new_block_info(&Default::default(), *number, 0);
            let raw_tx = RawL2Transaction::new_builder()
                .from_id(from_id.pack())
                .to_id(contract_id.pack())
                .args(Bytes::from(add_value.to_le_bytes().to_vec()).pack())
                .build();
            let run_result = generator
                .execute_transaction(&chain_view, &mut tree, &block_info, &raw_tx, None, None)
                .expect("construct");
            let return_value = {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&run_result.return_data);
                u64::from_le_bytes(buf)
            };
            sum_value += add_value;
            assert_eq!(return_value, sum_value);
            println!("result {:?}", run_result);
        }
    }
}

pub enum AccountOp {
    Load {
        account_id: u32,
        key: [u8; 32],
    },
    Store {
        account_id: u32,
        key: [u8; 32],
        value: [u8; 32],
    },
    LoadNonce {
        account_id: u32,
    },
    Log {
        account_id: u32,
        service_flag: u8,
        data: Vec<u8>,
    },
}

impl AccountOp {
    fn to_vec(&self) -> Vec<u8> {
        match self {
            AccountOp::Load { account_id, key } => {
                let mut data = vec![0xF0];
                data.extend(&account_id.to_le_bytes());
                data.extend(key);
                data
            }
            AccountOp::Store {
                account_id,
                key,
                value,
            } => {
                let mut data = vec![0xF1];
                data.extend(&account_id.to_le_bytes());
                data.extend(key);
                data.extend(value);
                data
            }
            AccountOp::LoadNonce { account_id } => {
                let mut data = vec![0xF2];
                data.extend(&account_id.to_le_bytes());
                data
            }
            AccountOp::Log {
                account_id,
                service_flag,
                data,
            } => {
                let mut args_data = vec![0xF3];
                args_data.extend(&account_id.to_le_bytes());
                args_data.push(*service_flag);
                args_data.extend(&(data.len() as u32).to_le_bytes());
                args_data.extend(data);
                args_data
            }
        }
    }
}

#[test]
fn test_example_account_operation() {
    let store = Store::open_tmp().unwrap();
    let mut tree = new_state(store.get_snapshot());
    let chain_view = DummyChainStore;

    let sender_script = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.pack())
        .args([1u8; 20].to_vec().pack())
        .hash_type(ScriptHashType::Type.into())
        .build();
    let from_id = tree
        .create_account_from_script(sender_script.clone())
        .expect("create account");
    tree.mapping_registry_address_to_script_hash(
        RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, vec![42u8; 20]),
        sender_script.hash().into(),
    )
    .unwrap();

    let contract_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash(ACCOUNT_OP_PROGRAM_CODE_HASH.pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");

    let backend_manage = BackendManage::from_config(vec![BackendForkConfig {
        fork_height: 0,
        backends: vec![BackendConfig {
            validator_path: ACCOUNT_OP_PROGRAM_PATH.clone(),
            generator_path: ACCOUNT_OP_PROGRAM_PATH.clone(),
            validator_script_type_hash: (*ACCOUNT_OP_PROGRAM_CODE_HASH).into(),
            backend_type: BackendType::Unknown,
        }],
    }])
    .unwrap();
    let mut account_lock_manage = AccountLockManage::default();
    account_lock_manage.register_lock_algorithm(H256::zero(), Arc::new(AlwaysSuccess::default()));
    let rollup_context = RollupContext {
        rollup_config: RollupConfig::new_builder()
            .allowed_contract_type_hashes(
                vec![AllowedTypeHash::new_builder()
                    .hash(ACCOUNT_OP_PROGRAM_CODE_HASH.pack())
                    .type_(AllowedContractType::Unknown.into())
                    .build()]
                .pack(),
            )
            .build(),
        rollup_script_hash: [42u8; 32].into(),
        ..Default::default()
    };
    let generator = Generator::new(
        backend_manage,
        account_lock_manage,
        rollup_context,
        Default::default(),
    );
    let block_info = new_block_info(&Default::default(), 2, 0);
    tree.finalise().unwrap();
    let tree = tree;

    // Load: success
    {
        let mut tree = tree.clone();
        let args = AccountOp::Load {
            account_id: 0,
            key: [1u8; 32],
        };
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .args(Bytes::from(args.to_vec()).pack())
            .build();
        let run_result = generator
            .execute_transaction(&chain_view, &mut tree, &block_info, &raw_tx, None, None)
            .expect("result");
        assert_eq!(run_result.return_data, vec![0u8; 32]);
    }
    // Load: account not found
    {
        let mut tree = tree.clone();
        let args = AccountOp::Load {
            account_id: 0xff33,
            key: [1u8; 32],
        };
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .args(Bytes::from(args.to_vec()).pack())
            .build();
        let err = generator
            .execute_transaction(&chain_view, &mut tree, &block_info, &raw_tx, None, None)
            .expect_err("err");
        let err_code = match err.downcast::<TransactionError>() {
            Ok(TransactionError::UnknownTxType(code)) => code,
            err => panic!("unexpected {:?}", err),
        };
        assert_eq!(err_code, GW_ERROR_ACCOUNT_NOT_FOUND as i8);
    }

    // Store: success
    {
        let mut tree = tree.clone();
        let args = AccountOp::Store {
            account_id: 0,
            key: [1u8; 32],
            value: [1u8; 32],
        };
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .args(Bytes::from(args.to_vec()).pack())
            .build();
        let run_result = generator
            .execute_transaction(&chain_view, &mut tree, &block_info, &raw_tx, None, None)
            .expect("result");
        assert_eq!(run_result.return_data, Vec::<u8>::new());
    }
    // Store: account not found
    {
        let mut tree = tree.clone();
        let args = AccountOp::Store {
            account_id: 0xff33,
            key: [1u8; 32],
            value: [1u8; 32],
        };
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .args(Bytes::from(args.to_vec()).pack())
            .build();
        let err = generator
            .execute_transaction(&chain_view, &mut tree, &block_info, &raw_tx, None, None)
            .expect_err("err");
        let err_code = match err.downcast::<TransactionError>() {
            Ok(TransactionError::UnknownTxType(code)) => code,
            err => panic!("unexpected {:?}", err),
        };
        assert_eq!(err_code, GW_ERROR_ACCOUNT_NOT_FOUND as i8);
    }

    // LoadNonce: success
    {
        let mut tree = tree.clone();
        let args = AccountOp::LoadNonce { account_id: 0 };
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .args(Bytes::from(args.to_vec()).pack())
            .build();
        let run_result = generator
            .execute_transaction(&chain_view, &mut tree, &block_info, &raw_tx, None, None)
            .expect("result");
        assert_eq!(run_result.return_data, 0u32.to_le_bytes().to_vec());
    }
    // LoadNonce: account not found
    {
        let mut tree = tree.clone();
        let args = AccountOp::LoadNonce { account_id: 0xff33 };
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .args(Bytes::from(args.to_vec()).pack())
            .build();
        let err = generator
            .execute_transaction(&chain_view, &mut tree, &block_info, &raw_tx, None, None)
            .expect_err("err");
        let err_code = match err.downcast::<TransactionError>() {
            Ok(TransactionError::UnknownTxType(code)) => code,
            err => panic!("unexpected {:?}", err),
        };
        assert_eq!(err_code, GW_ERROR_ACCOUNT_NOT_FOUND as i8);
    }

    // Log: success
    {
        let mut tree = tree.clone();
        let account_id = 0;
        let registry_id = ETH_REGISTRY_ACCOUNT_ID;
        let from_addr = RegistryAddress::new(registry_id, vec![0x33u8; 20]);
        let to_addr = RegistryAddress::new(registry_id, vec![0x44u8; 20]);
        let amount: U256 = 101u64.into();
        let mut buf = [0u8; 32];
        amount.to_little_endian(&mut buf);
        let mut data = Vec::default();
        data.extend(from_addr.to_bytes());
        data.extend(to_addr.to_bytes());
        data.extend(&buf);
        let args = AccountOp::Log {
            service_flag: GW_LOG_SUDT_TRANSFER,
            account_id,
            data,
        };
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .args(Bytes::from(args.to_vec()).pack())
            .build();
        let run_result = generator
            .execute_transaction(&chain_view, &mut tree, &block_info, &raw_tx, None, None)
            .expect("result");
        let log = SudtLog::from_log_item(&run_result.logs[0]).unwrap();
        assert_eq!(log.sudt_id, account_id);
        assert_eq!(log.from_addr, from_addr);
        assert_eq!(log.to_addr, to_addr);
        assert_eq!(log.amount, amount);
        assert_eq!(log.log_type, SudtLogType::Transfer);
        assert_eq!(run_result.return_data, Vec::<u8>::new());
    }
    // Log: account not found
    {
        let mut tree = tree;
        let args = AccountOp::Log {
            account_id: 0xff33,
            service_flag: GW_LOG_SUDT_TRANSFER,
            data: vec![3u8; 22],
        };
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .args(Bytes::from(args.to_vec()).pack())
            .build();
        let err = generator
            .execute_transaction(&chain_view, &mut tree, &block_info, &raw_tx, None, None)
            .expect_err("err");
        let err_code = match err.downcast::<TransactionError>() {
            Ok(TransactionError::UnknownTxType(code)) => code,
            err => panic!("unexpected {:?}", err),
        };
        assert_eq!(err_code, GW_ERROR_ACCOUNT_NOT_FOUND as i8);
    }
}

#[test]
fn test_example_recover_account() {
    let store = Store::open_tmp().unwrap();
    let mut tree = new_state(store.get_snapshot());
    let chain_view = DummyChainStore;

    let sender_script = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.pack())
        .args([1u8; 20].to_vec().pack())
        .hash_type(ScriptHashType::Type.into())
        .build();
    let from_id = tree
        .create_account_from_script(sender_script.clone())
        .expect("create account");
    tree.mapping_registry_address_to_script_hash(
        RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, vec![42u8; 20]),
        sender_script.hash().into(),
    )
    .unwrap();

    let contract_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash(RECOVER_PROGRAM_CODE_HASH.pack())
                .args([42u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");

    let backend_manage = BackendManage::from_config(vec![BackendForkConfig {
        fork_height: 0,
        backends: vec![BackendConfig {
            validator_path: RECOVER_PROGRAM_PATH.clone(),
            generator_path: RECOVER_PROGRAM_PATH.clone(),
            validator_script_type_hash: (*RECOVER_PROGRAM_CODE_HASH).into(),
            backend_type: BackendType::Unknown,
        }],
    }])
    .unwrap();
    let mut account_lock_manage = AccountLockManage::default();
    let secp256k1_code_hash = H256::from_u32(11);
    account_lock_manage
        .register_lock_algorithm(secp256k1_code_hash, Arc::new(Secp256k1Eth::default()));
    let rollup_script_hash: H256 = [42u8; 32].into();
    let rollup_context = RollupContext {
        rollup_config: RollupConfig::new_builder()
            .allowed_contract_type_hashes(
                vec![AllowedTypeHash::new_builder()
                    .hash(RECOVER_PROGRAM_CODE_HASH.pack())
                    .type_(AllowedContractType::Unknown.into())
                    .build()]
                .pack(),
            )
            .build(),
        rollup_script_hash,
        ..Default::default()
    };
    let generator = Generator::new(
        backend_manage,
        account_lock_manage,
        rollup_context,
        Default::default(),
    );
    let block_info = new_block_info(&Default::default(), 2, 0);

    let lock_args_hex = "0be314c65ef1a40deab86811419ad7b7219686eb";
    let message_hex = "879a053d4800c6354e76c7985a865d2922c82fb5b3f4577b2fe08b998954f2e0";
    let signature_hex = "04d094cc31b6989a9e76c2e3964f5e0ee71b9041a1ce442561e9fdaeae67874d3b4499d8b65fd6d4d76677d72cba39aa25cb5c8d0d9feb52f638fc4d4cda9c021b";

    // success
    {
        let mut args = vec![0u8; 32 + 1 + 65 + 32];
        args[0..32].copy_from_slice(&hex::decode(message_hex).unwrap());
        args[32] = 65;
        args[33..33 + 65].copy_from_slice(&hex::decode(signature_hex).unwrap());
        args[33 + 65..33 + 65 + 32].copy_from_slice(secp256k1_code_hash.as_slice());
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .args(Bytes::from(args).pack())
            .build();
        let run_result = generator
            .execute_transaction(&chain_view, &mut tree, &block_info, &raw_tx, None, None)
            .expect("result");
        let mut script_args = vec![0u8; 32 + 20];
        script_args[0..32].copy_from_slice(rollup_script_hash.as_slice());
        script_args[32..32 + 20].copy_from_slice(&hex::decode(lock_args_hex).unwrap());
        let script = Script::new_builder()
            .code_hash(secp256k1_code_hash.pack())
            .hash_type(ScriptHashType::Type.into())
            .args(Bytes::from(script_args).pack())
            .build();
        let ret_script = Script::from_slice(&run_result.return_data).unwrap();
        assert_eq!(ret_script, script);
    }

    // Error signature
    {
        let mut args = vec![0u8; 32 + 1 + 65 + 32];
        let error_signature_hex = "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
        args[0..32].copy_from_slice(&hex::decode(message_hex).unwrap());
        args[32] = 65;
        args[33..33 + 65].copy_from_slice(&hex::decode(error_signature_hex).unwrap());
        args[33 + 65..33 + 65 + 32].copy_from_slice(secp256k1_code_hash.as_slice());
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .args(Bytes::from(args).pack())
            .build();
        let err = generator
            .execute_transaction(&chain_view, &mut tree, &block_info, &raw_tx, None, None)
            .expect_err("err");
        let err_code = match err.downcast::<TransactionError>() {
            Ok(TransactionError::UnknownTxType(code)) => code,
            err => panic!("unexpected {:?}", err),
        };
        println!("err_code: {}", err_code);
        assert_eq!(err_code, GW_ERROR_RECOVER as i8);
    }

    // Wrong code hash
    {
        let mut args = vec![0u8; 32 + 1 + 65 + 32];
        let wrong_code_hash = H256::from_u32(22);
        args[0..32].copy_from_slice(&hex::decode(message_hex).unwrap());
        args[32] = 65;
        args[33..33 + 65].copy_from_slice(&hex::decode(signature_hex).unwrap());
        args[33 + 65..33 + 65 + 32].copy_from_slice(wrong_code_hash.as_slice());
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .args(Bytes::from(args).pack())
            .build();
        let err = generator
            .execute_transaction(&chain_view, &mut tree, &block_info, &raw_tx, None, None)
            .expect_err("err");
        let err_code = match err.downcast::<TransactionError>() {
            Ok(TransactionError::UnknownTxType(code)) => code,
            err => panic!("unexpected {:?}", err),
        };
        println!("err_code: {}", err_code);
        assert_eq!(err_code, GW_FATAL_UNKNOWN_ARGS);
    }
}

#[test]
fn test_sudt_total_supply() {
    let store = Store::open_tmp().unwrap();
    let mut tree = new_state(store.get_snapshot());
    let chain_view = DummyChainStore;
    let rollup_config = RollupConfig::new_builder()
        .l2_sudt_validator_script_type_hash(ALWAYS_SUCCESS_CODE_HASH.pack())
        .allowed_contract_type_hashes(
            vec![AllowedTypeHash::new_builder()
                .hash(SUDT_TOTAL_SUPPLY_PROGRAM_CODE_HASH.pack())
                .type_(AllowedContractType::Unknown.into())
                .build()]
            .pack(),
        )
        .build();

    let sudt_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash(ALWAYS_SUCCESS_CODE_HASH.pack())
                .args([0u8; 32].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create sudt id");

    let alice = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.pack())
        .args([1u8; 32].to_vec().pack())
        .hash_type(ScriptHashType::Type.into())
        .build();
    let alice_hash: H256 = alice.hash().into();
    let eth_registry_id = gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID;
    let alice_address = RegistryAddress::new(eth_registry_id, alice_hash.as_slice().to_vec());
    let alice_id = tree
        .create_account_from_script(alice)
        .expect("create alice account");
    tree.mint_sudt(sudt_id, &alice_address, u128::MAX.into())
        .expect("alice mint sudt");

    let bob = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.pack())
        .args([2u8; 32].to_vec().pack())
        .hash_type(ScriptHashType::Type.into())
        .build();
    let bob_hash: H256 = bob.hash().into();
    let bob_address = RegistryAddress::new(eth_registry_id, bob_hash.as_slice().to_vec());
    tree.create_account_from_script(bob)
        .expect("create bob account");
    tree.mint_sudt(sudt_id, &bob_address, u128::MAX.into())
        .expect("bob mint sudt");

    let contract_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash(SUDT_TOTAL_SUPPLY_PROGRAM_CODE_HASH.pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create contract account");

    // run handle message
    {
        // NOTICE in this test we won't need SUM validator
        let backend_manage = BackendManage::from_config(vec![BackendForkConfig {
            fork_height: 0,
            backends: vec![BackendConfig {
                validator_path: SUDT_TOTAL_SUPPLY_PROGRAM_PATH.clone(),
                generator_path: SUDT_TOTAL_SUPPLY_PROGRAM_PATH.clone(),
                validator_script_type_hash: (*SUDT_TOTAL_SUPPLY_PROGRAM_CODE_HASH).into(),
                backend_type: BackendType::Unknown,
            }],
        }])
        .unwrap();
        let mut account_lock_manage = AccountLockManage::default();
        account_lock_manage.register_lock_algorithm(
            (*ALWAYS_SUCCESS_CODE_HASH).into(),
            Arc::new(AlwaysSuccess::default()),
        );
        let rollup_context = RollupContext {
            rollup_config,
            rollup_script_hash: [42u8; 32].into(),
            ..Default::default()
        };
        let generator = Generator::new(
            backend_manage,
            account_lock_manage,
            rollup_context,
            Default::default(),
        );
        let block_info = new_block_info(&Default::default(), 1, 0);
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(alice_id.pack())
            .to_id(contract_id.pack())
            .args(Bytes::from(sudt_id.to_le_bytes().to_vec()).pack())
            .build();
        let run_result = generator
            .execute_transaction(&chain_view, &mut tree, &block_info, &raw_tx, None, None)
            .expect("construct");
        let return_value = {
            let mut buf = [0u8; 32];
            buf.copy_from_slice(&run_result.return_data);
            U256::from_little_endian(&buf)
        };
        assert_eq!(return_value, U256::from(u128::MAX) + U256::from(u128::MAX));
        println!("result {:?}", return_value);
    }
}
