use super::super::utils::init_env_log;
use crate::script_tests::utils::context::TestingContext;
use crate::testing_tool::chain::SUDT_VALIDATOR_SCRIPT_TYPE_HASH;

use super::{check_transfer_logs, new_block_info, run_contract, run_contract_get_result};
use ckb_vm::Bytes;
use gw_common::builtins::CKB_SUDT_ACCOUNT_ID;
use gw_common::registry_address::RegistryAddress;
use gw_common::state::State;
use gw_generator::syscalls::error_codes::{
    GW_SUDT_ERROR_AMOUNT_OVERFLOW, GW_SUDT_ERROR_INSUFFICIENT_BALANCE,
};
use gw_generator::{error::TransactionError, traits::StateExt};
use gw_store::state::traits::JournalDB;
use gw_traits::CodeStore;
use gw_types::packed::{BlockInfo, Fee};
use gw_types::U256;
use gw_types::{
    core::ScriptHashType,
    packed::{RollupConfig, SUDTArgs, SUDTQuery, SUDTTransfer, Script},
    prelude::*,
};

#[test]
fn test_sudt() {
    init_env_log();
    let rollup_config = TestingContext::default_rollup_config()
        .as_builder()
        .l2_sudt_validator_script_type_hash(SUDT_VALIDATOR_SCRIPT_TYPE_HASH.pack())
        .build();
    let mut ctx = TestingContext::setup_with_config(rollup_config);

    let init_a_balance = U256::from(10000u64);

    // init accounts
    let _meta = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([1u8; 64].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let sudt_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([0u8; 64].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([1u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_script_hash = ctx.state.get_script_hash(a_id).expect("get script hash");
    let b_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([2u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let b_script_hash = ctx.state.get_script_hash(b_id).expect("get script hash");
    let block_producer_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([42u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let block_producer_script_hash = ctx
        .state
        .get_script_hash(block_producer_id)
        .expect("get script hash");
    let block_producer = ctx.create_eth_address(block_producer_script_hash, [42u8; 20]);
    let block_info = new_block_info(&block_producer, 1, 0);

    let a_address = ctx.create_eth_address(a_script_hash, [1u8; 20]);
    let b_address = ctx.create_eth_address(b_script_hash, [2u8; 20]);

    // init balance for a
    ctx.state
        .mint_sudt(sudt_id, &a_address, init_a_balance)
        .expect("init balance");

    // init ckb for a to pay fee
    let init_ckb: U256 = 100u64.into();
    ctx.state
        .mint_sudt(CKB_SUDT_ACCOUNT_ID, &a_address, init_ckb)
        .expect("init balance");

    ctx.state.finalise().unwrap();
    let ctx = ctx;

    // check balance of A, B
    {
        let mut state = ctx.state.clone();
        check_balance(
            &ctx.rollup_config,
            &mut state,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance,
        );

        check_balance(
            &ctx.rollup_config,
            &mut state,
            &block_info,
            a_id,
            sudt_id,
            &b_address,
            U256::zero(),
        );
    }

    // transfer from A to B
    {
        let mut state = ctx.state.clone();
        let value: U256 = 4000u128.into();
        let fee = 42u128;
        let sender_nonce = state.get_nonce(a_id).unwrap();
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to_address(Bytes::from(b_address.to_bytes()).pack())
                    .amount(value.pack())
                    .fee(
                        Fee::new_builder()
                            .amount(fee.pack())
                            .registry_id(a_address.registry_id.pack())
                            .build(),
                    )
                    .build(),
            )
            .build();
        let run_result = run_contract_get_result(
            &ctx.rollup_config,
            &mut state,
            a_id,
            sudt_id,
            args.as_bytes(),
            &block_info,
        )
        .expect("execute");
        let new_sender_nonce = state.get_nonce(a_id).unwrap();
        assert_eq!(sender_nonce + 1, new_sender_nonce, "nonce increased");
        assert!(run_result.return_data.is_empty());
        assert_eq!(run_result.logs.len(), 2);
        check_transfer_logs(
            &run_result.logs,
            sudt_id,
            &block_producer,
            fee,
            &a_address,
            &b_address,
            value,
        );

        {
            // check sender's sudt
            check_balance(
                &ctx.rollup_config,
                &mut state,
                &block_info,
                a_id,
                sudt_id,
                &a_address,
                init_a_balance - value,
            );

            // check sender's ckb
            check_balance(
                &ctx.rollup_config,
                &mut state,
                &block_info,
                a_id,
                CKB_SUDT_ACCOUNT_ID,
                &a_address,
                init_ckb - fee,
            );

            // check receiver's sudt
            check_balance(
                &ctx.rollup_config,
                &mut state,
                &block_info,
                a_id,
                sudt_id,
                &b_address,
                value,
            );

            // check receiver's ckb
            check_balance(
                &ctx.rollup_config,
                &mut state,
                &block_info,
                a_id,
                CKB_SUDT_ACCOUNT_ID,
                &b_address,
                U256::zero(),
            );

            // check producers's sudt
            check_balance(
                &ctx.rollup_config,
                &mut state,
                &block_info,
                a_id,
                sudt_id,
                &block_producer,
                U256::zero(),
            );

            // check producers's ckb
            check_balance(
                &ctx.rollup_config,
                &mut state,
                &block_info,
                a_id,
                CKB_SUDT_ACCOUNT_ID,
                &block_producer,
                fee.into(),
            );
        }
    }
}

#[test]
fn test_insufficient_balance() {
    init_env_log();
    let init_a_balance = U256::from(10000u32);

    let rollup_config = TestingContext::default_rollup_config()
        .as_builder()
        .l2_sudt_validator_script_type_hash(SUDT_VALIDATOR_SCRIPT_TYPE_HASH.pack())
        .build();
    let mut ctx = TestingContext::setup_with_config(rollup_config);

    // init accounts
    let _meta = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([1u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let sudt_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");

    let a_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_script_hash = ctx.state.get_script_hash(a_id).expect("get script hash");
    let b_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([1u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let b_script_hash = ctx.state.get_script_hash(b_id).expect("get script hash");

    let block_info = new_block_info(&Default::default(), 10, 0);

    let a_address = ctx.create_eth_address(a_script_hash, [1u8; 20]);
    let b_address = ctx.create_eth_address(b_script_hash, [2u8; 20]);
    // init balance for a
    ctx.state
        .mint_sudt(sudt_id, &a_address, init_a_balance)
        .expect("init balance");

    ctx.state.finalise().unwrap();
    let ctx = ctx;

    // transfer from A to B
    {
        let mut state = ctx.state.clone();
        let value: U256 = 10001u128.into();
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to_address(Bytes::from(b_address.to_bytes()).pack())
                    .amount(value.pack())
                    .fee(
                        Fee::new_builder()
                            .registry_id(a_address.registry_id.pack())
                            .build(),
                    )
                    .build(),
            )
            .build();
        let result = run_contract_get_result(
            &ctx.rollup_config,
            &mut state,
            a_id,
            sudt_id,
            args.as_bytes(),
            &block_info,
        )
        .unwrap();
        assert_eq!(result.exit_code, GW_SUDT_ERROR_INSUFFICIENT_BALANCE);
    }
}

#[test]
fn test_transfer_to_non_exist_account() {
    let init_a_balance = U256::from(10000u32);

    let rollup_config = TestingContext::default_rollup_config()
        .as_builder()
        .l2_sudt_validator_script_type_hash(SUDT_VALIDATOR_SCRIPT_TYPE_HASH.pack())
        .build();
    let mut ctx = TestingContext::setup_with_config(rollup_config);

    // init accounts
    let _meta = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([1u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let sudt_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_script_hash = ctx.state.get_script_hash(a_id).expect("get script hash");
    // non-exist account id
    let a_address = ctx.create_eth_address(a_script_hash, [1u8; 20]);
    let b_address = RegistryAddress::new(a_address.registry_id, [0x33u8; 20].to_vec());

    let block_info = new_block_info(&Default::default(), 10, 0);

    // init balance for a
    ctx.state
        .mint_sudt(sudt_id, &a_address, init_a_balance)
        .expect("init balance");

    ctx.state.finalise().unwrap();
    let ctx = ctx;

    // transfer from A to B
    {
        let mut state = ctx.state.clone();
        let value: U256 = 1000u64.into();
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to_address(Bytes::from(b_address.to_bytes()).pack())
                    .amount(value.pack())
                    .fee(
                        Fee::new_builder()
                            .registry_id(a_address.registry_id.pack())
                            .build(),
                    )
                    .build(),
            )
            .build();
        let _run_result = run_contract(
            &ctx.rollup_config,
            &mut state,
            a_id,
            sudt_id,
            args.as_bytes(),
            &block_info,
        )
        .expect("run contract");
    }
}

#[test]
fn test_transfer_to_self() {
    let init_a_balance = U256::from(10000u64);
    let init_ckb: U256 = 100u64.into();

    let rollup_config = TestingContext::default_rollup_config()
        .as_builder()
        .l2_sudt_validator_script_type_hash(SUDT_VALIDATOR_SCRIPT_TYPE_HASH.pack())
        .build();
    let mut ctx = TestingContext::setup_with_config(rollup_config);

    // init accounts
    let _meta = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([1u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let sudt_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_script_hash = ctx.state.get_script_hash(a_id).expect("get script hash");
    // non-exist account id
    let a_address = ctx.create_eth_address(a_script_hash, [1u8; 20]);

    let block_producer_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([2u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let block_producer_script_hash = ctx
        .state
        .get_script_hash(block_producer_id)
        .expect("get script hash");
    let block_producer = ctx.create_eth_address(block_producer_script_hash, [42u8; 20]);
    let block_producer_balance = U256::zero();
    let block_info = new_block_info(&block_producer, 10, 0);

    // init balance for a
    ctx.state
        .mint_sudt(sudt_id, &a_address, init_a_balance)
        .expect("init balance");

    ctx.state
        .mint_sudt(CKB_SUDT_ACCOUNT_ID, &a_address, init_ckb)
        .expect("init balance");

    ctx.state.finalise().unwrap();
    let state = &mut ctx.state;

    // transfer from A to A, zero value
    {
        let value = U256::zero();
        let fee = 0u128;
        let sender_nonce = state.get_nonce(a_id).unwrap();
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to_address(Bytes::from(a_address.to_bytes()).pack())
                    .amount(value.pack())
                    .fee(
                        Fee::new_builder()
                            .amount(fee.pack())
                            .registry_id(a_address.registry_id.pack())
                            .build(),
                    )
                    .build(),
            )
            .build();
        let run_result = run_contract_get_result(
            &ctx.rollup_config,
            state,
            a_id,
            sudt_id,
            args.as_bytes(),
            &block_info,
        )
        .expect("run contract");
        let new_sender_nonce = state.get_nonce(a_id).unwrap();
        assert_eq!(sender_nonce + 1, new_sender_nonce, "nonce increased");
        assert_eq!(run_result.logs.len(), 2);
        check_transfer_logs(
            &run_result.logs,
            sudt_id,
            &block_producer,
            fee,
            &a_address,
            &a_address,
            value,
        );
        check_balance(
            &ctx.rollup_config,
            state,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance,
        );
        check_balance(
            &ctx.rollup_config,
            state,
            &block_info,
            a_id,
            sudt_id,
            &block_producer,
            block_producer_balance,
        );
        state.finalise().unwrap();
    }

    // transfer from A to A, normal value
    let fee = 20u128;
    {
        let value: U256 = 1000u64.into();
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to_address(Bytes::from(a_address.to_bytes()).pack())
                    .amount(value.pack())
                    .fee(
                        Fee::new_builder()
                            .amount(fee.pack())
                            .registry_id(a_address.registry_id.pack())
                            .build(),
                    )
                    .build(),
            )
            .build();
        let run_result = run_contract_get_result(
            &ctx.rollup_config,
            state,
            a_id,
            sudt_id,
            args.as_bytes(),
            &block_info,
        )
        .expect("run contract");
        assert_eq!(run_result.logs.len(), 2);
        check_transfer_logs(
            &run_result.logs,
            sudt_id,
            &block_producer,
            fee,
            &a_address,
            &a_address,
            value,
        );

        // sender's sudt balance
        check_balance(
            &ctx.rollup_config,
            state,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance,
        );

        // sender's ckb balance
        check_balance(
            &ctx.rollup_config,
            state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &a_address,
            init_ckb - fee,
        );

        // block producer's balance
        check_balance(
            &ctx.rollup_config,
            state,
            &block_info,
            a_id,
            sudt_id,
            &block_producer,
            block_producer_balance,
        );

        // block producer's balance
        check_balance(
            &ctx.rollup_config,
            state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &block_producer,
            fee.into(),
        );
        state.finalise().unwrap();
    }

    // transfer from A to A, insufficient balance
    {
        let value: U256 = 100000u64.into();
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to_address(Bytes::from(a_address.to_bytes()).pack())
                    .amount(value.pack())
                    .fee(
                        Fee::new_builder()
                            .registry_id(a_address.registry_id.pack())
                            .build(),
                    )
                    .build(),
            )
            .build();
        let result = run_contract_get_result(
            &ctx.rollup_config,
            state,
            a_id,
            sudt_id,
            args.as_bytes(),
            &block_info,
        )
        .unwrap();
        assert_eq!(result.exit_code, GW_SUDT_ERROR_INSUFFICIENT_BALANCE);
        // sender sudt
        check_balance(
            &ctx.rollup_config,
            state,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance,
        );

        // sender's ckb
        check_balance(
            &ctx.rollup_config,
            state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &a_address,
            init_ckb - fee,
        );
        // block producer ckb
        check_balance(
            &ctx.rollup_config,
            state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &block_producer,
            fee.into(),
        );
        state.finalise().unwrap();
    }
}

#[test]
fn test_transfer_to_self_overflow() {
    let init_a_balance: U256 = U256::MAX - U256::one();
    let init_ckb = U256::from(100u64);

    let rollup_config = TestingContext::default_rollup_config()
        .as_builder()
        .l2_sudt_validator_script_type_hash(SUDT_VALIDATOR_SCRIPT_TYPE_HASH.pack())
        .build();
    let mut ctx = TestingContext::setup_with_config(rollup_config);

    // init accounts
    let _meta = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([1u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let sudt_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_script_hash = ctx.state.get_script_hash(a_id).expect("get script hash");
    // non-exist account id
    let a_address = ctx.create_eth_address(a_script_hash, [1u8; 20]);

    let block_producer_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([2u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let block_producer_script_hash = ctx
        .state
        .get_script_hash(block_producer_id)
        .expect("get script hash");
    let block_producer = ctx.create_eth_address(block_producer_script_hash, [42u8; 20]);
    let block_producer_balance = U256::zero();
    let block_info = new_block_info(&block_producer, 10, 0);

    // init balance for a
    ctx.state
        .mint_sudt(sudt_id, &a_address, init_a_balance)
        .expect("init balance");
    ctx.state
        .mint_sudt(CKB_SUDT_ACCOUNT_ID, &a_address, init_ckb)
        .expect("init balance");

    ctx.state.finalise().unwrap();
    let state = &mut ctx.state;

    // transfer from A to A, zero value
    {
        let value = U256::zero();
        let fee = 0u128;
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to_address(Bytes::from(a_address.to_bytes()).pack())
                    .amount(value.pack())
                    .fee(
                        Fee::new_builder()
                            .amount(fee.pack())
                            .registry_id(a_address.registry_id.pack())
                            .build(),
                    )
                    .build(),
            )
            .build();
        let run_result = run_contract_get_result(
            &ctx.rollup_config,
            state,
            a_id,
            sudt_id,
            args.as_bytes(),
            &block_info,
        )
        .expect("run contract");
        assert_eq!(run_result.logs.len(), 2);
        check_transfer_logs(
            &run_result.logs,
            sudt_id,
            &block_producer,
            fee,
            &a_address,
            &a_address,
            value,
        );

        // sender's sudt
        check_balance(
            &ctx.rollup_config,
            state,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance,
        );
        // sender's ckb
        check_balance(
            &ctx.rollup_config,
            state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &a_address,
            init_ckb,
        );
        // block producer's sudt
        check_balance(
            &ctx.rollup_config,
            state,
            &block_info,
            a_id,
            sudt_id,
            &block_producer,
            block_producer_balance,
        );
        // block producer's ckb
        check_balance(
            &ctx.rollup_config,
            state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &block_producer,
            U256::zero(),
        );
        state.finalise().unwrap();
    }

    // transfer from A to A, 1 value
    {
        let value = U256::one();
        let fee = 0u128;
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to_address(Bytes::from(a_address.to_bytes()).pack())
                    .amount(value.pack())
                    .fee(
                        Fee::new_builder()
                            .amount(fee.pack())
                            .registry_id(a_address.registry_id.pack())
                            .build(),
                    )
                    .build(),
            )
            .build();
        let run_result = run_contract_get_result(
            &ctx.rollup_config,
            state,
            a_id,
            sudt_id,
            args.as_bytes(),
            &block_info,
        )
        .expect("run contract");
        assert_eq!(run_result.logs.len(), 2);
        check_transfer_logs(
            &run_result.logs,
            sudt_id,
            &block_producer,
            fee,
            &a_address,
            &a_address,
            value,
        );
        // sudt
        check_balance(
            &ctx.rollup_config,
            state,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance,
        );
        check_balance(
            &ctx.rollup_config,
            state,
            &block_info,
            a_id,
            sudt_id,
            &block_producer,
            block_producer_balance,
        );
        // ckb
        check_balance(
            &ctx.rollup_config,
            state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &a_address,
            init_ckb,
        );
        check_balance(
            &ctx.rollup_config,
            state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &block_producer,
            U256::zero(),
        );
        state.finalise().unwrap();
    }

    // transfer from A to A, overflow balance
    {
        let value: U256 = 100000u64.into();
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to_address(Bytes::from(a_address.to_bytes()).pack())
                    .amount(value.pack())
                    .fee(
                        Fee::new_builder()
                            .registry_id(a_address.registry_id.pack())
                            .build(),
                    )
                    .build(),
            )
            .build();
        let run_result = run_contract_get_result(
            &ctx.rollup_config,
            state,
            a_id,
            sudt_id,
            args.as_bytes(),
            &block_info,
        )
        .expect("ok");
        assert_eq!(run_result.logs.len(), 2);
        check_transfer_logs(
            &run_result.logs,
            sudt_id,
            &block_producer,
            0,
            &a_address,
            &a_address,
            value,
        );
        // sudt
        check_balance(
            &ctx.rollup_config,
            state,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance,
        );
        check_balance(
            &ctx.rollup_config,
            state,
            &block_info,
            a_id,
            sudt_id,
            &block_producer,
            block_producer_balance,
        );
        // ckb
        check_balance(
            &ctx.rollup_config,
            state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &a_address,
            init_ckb,
        );
        check_balance(
            &ctx.rollup_config,
            state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &block_producer,
            U256::zero(),
        );
        state.finalise().unwrap();
    }

    // transfer from A to A with a large value
    {
        let value: U256 = U256::MAX - U256::one();
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to_address(Bytes::from(a_address.to_bytes()).pack())
                    .amount(value.pack())
                    .fee(
                        Fee::new_builder()
                            .registry_id(a_address.registry_id.pack())
                            .build(),
                    )
                    .build(),
            )
            .build();
        let run_result = run_contract_get_result(
            &ctx.rollup_config,
            state,
            a_id,
            sudt_id,
            args.as_bytes(),
            &block_info,
        )
        .expect("ok");
        assert_eq!(run_result.logs.len(), 2);
        check_transfer_logs(
            &run_result.logs,
            sudt_id,
            &block_producer,
            0,
            &a_address,
            &a_address,
            value,
        );
        //sudt
        check_balance(
            &ctx.rollup_config,
            state,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance,
        );
        check_balance(
            &ctx.rollup_config,
            state,
            &block_info,
            a_id,
            sudt_id,
            &block_producer,
            block_producer_balance,
        );
        //ckb
        check_balance(
            &ctx.rollup_config,
            state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &a_address,
            init_ckb,
        );
        check_balance(
            &ctx.rollup_config,
            state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &block_producer,
            U256::zero(),
        );
        state.finalise().unwrap();
    }
}

// Total supply overflow
#[test]
#[ignore]
fn test_transfer_overflow() {
    let init_a_balance = U256::from(10000u64);
    let init_b_balance: U256 = U256::MAX - init_a_balance;
    let init_a_ckb = U256::from(100u64);

    let rollup_config = TestingContext::default_rollup_config()
        .as_builder()
        .l2_sudt_validator_script_type_hash(SUDT_VALIDATOR_SCRIPT_TYPE_HASH.pack())
        .build();
    let mut ctx = TestingContext::setup_with_config(rollup_config);

    // init accounts
    let _meta = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([1u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let sudt_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_script_hash = ctx.state.get_script_hash(a_id).expect("get script hash");
    let a_address = ctx.create_eth_address(a_script_hash, [1u8; 20]);
    let b_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([1u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let b_script_hash = ctx.state.get_script_hash(b_id).expect("get script hash");
    let b_address = ctx.create_eth_address(b_script_hash, [2u8; 20]);

    let block_info = new_block_info(&Default::default(), 10, 0);

    // init balance for a
    ctx.state
        .mint_sudt(sudt_id, &a_address, init_a_balance)
        .expect("init balance");
    ctx.state
        .mint_sudt(CKB_SUDT_ACCOUNT_ID, &a_address, init_a_ckb)
        .expect("init balance");
    ctx.state
        .mint_sudt(sudt_id, &b_address, init_b_balance)
        .expect("init balance");

    ctx.state.finalise().unwrap();
    let ctx = ctx;
    // transfer from A to B overflow
    {
        let mut state = ctx.state.clone();
        let value: U256 = 10000u64.into();
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to_address(Bytes::from(b_address.to_bytes()).pack())
                    .amount(value.pack())
                    .fee(
                        Fee::new_builder()
                            .registry_id(a_address.registry_id.pack())
                            .build(),
                    )
                    .build(),
            )
            .build();
        let err = run_contract(
            &ctx.rollup_config,
            &mut state,
            a_id,
            sudt_id,
            args.as_bytes(),
            &block_info,
        )
        .expect_err("err");
        let err_code = match err {
            TransactionError::InvalidExitCode(code) => code,
            err => panic!("unexpected {:?}", err),
        };
        assert_eq!(err_code, GW_SUDT_ERROR_AMOUNT_OVERFLOW);

        // check balance
        check_balance(
            &ctx.rollup_config,
            &mut state,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance,
        );

        check_balance(
            &ctx.rollup_config,
            &mut state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &a_address,
            init_a_ckb,
        );

        check_balance(
            &ctx.rollup_config,
            &mut state,
            &block_info,
            a_id,
            sudt_id,
            &b_address,
            init_b_balance,
        );
    }
}

fn check_balance<S: State + CodeStore + JournalDB>(
    rollup_config: &RollupConfig,
    tree: &mut S,
    block_info: &BlockInfo,
    sender_id: u32,
    sudt_id: u32,
    address: &RegistryAddress,
    expected_balance: U256,
) {
    // check balance
    let args = SUDTArgs::new_builder()
        .set(
            SUDTQuery::new_builder()
                .address(Bytes::from(address.to_bytes()).pack())
                .build(),
        )
        .build();
    let return_data = run_contract(
        rollup_config,
        tree,
        sender_id,
        sudt_id,
        args.as_bytes(),
        block_info,
    )
    .expect("execute");
    let balance = {
        let mut buf = [0u8; 32];
        buf.copy_from_slice(&return_data);
        U256::from_little_endian(&buf)
    };
    assert_eq!(balance, expected_balance);
}
