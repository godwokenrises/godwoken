use super::super::utils::init_env_log;
use super::{new_block_info, run_contract};
use crate::script_tests::l2_scripts::run_contract_get_result;
use crate::script_tests::utils::context::TestingContext;
use crate::testing_tool::chain::ALWAYS_SUCCESS_CODE_HASH;
use gw_common::{
    builtins::{CKB_SUDT_ACCOUNT_ID, RESERVED_ACCOUNT_ID},
    state::State,
    H256,
};
use gw_generator::{
    error::TransactionError, syscalls::error_codes::GW_ERROR_DUPLICATED_SCRIPT_HASH,
    traits::StateExt,
};
use gw_types::U256;
use gw_types::{
    core::ScriptHashType,
    packed::{CreateAccount, Fee, MetaContractArgs, Script},
    prelude::*,
};

#[test]
fn test_meta_contract() {
    let mut ctx = TestingContext::setup();

    let a_script = Script::new_builder()
        .code_hash([0u8; 32].pack())
        .args([0u8; 20].to_vec().pack())
        .hash_type(ScriptHashType::Type.into())
        .build();
    let a_script_hash = a_script.hash();
    let a_id = ctx
        .state
        .create_account_from_script(a_script)
        .expect("create account");
    let a_address = ctx.create_eth_address(a_script_hash.into(), [1u8; 20]);
    ctx.state
        .mint_sudt(CKB_SUDT_ACCOUNT_ID, &a_address, U256::from(2000u64))
        .expect("mint CKB for account A to pay fee");

    let block_info = new_block_info(&a_address, 1, 0);

    // create contract
    let contract_script = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.pack())
        .hash_type(ScriptHashType::Type.into())
        .args([42u8; 33].pack())
        .build();
    let args = MetaContractArgs::new_builder()
        .set(
            CreateAccount::new_builder()
                .script(contract_script.clone())
                .fee(
                    Fee::new_builder()
                        .amount(1000u128.pack())
                        .registry_id(ctx.eth_registry_id.pack())
                        .build(),
                )
                .build(),
        )
        .build();
    let sender_nonce = ctx.state.get_nonce(a_id).unwrap();
    let return_data = run_contract(
        &ctx.rollup_config,
        &mut ctx.state,
        a_id,
        RESERVED_ACCOUNT_ID,
        args.as_bytes(),
        &block_info,
    )
    .expect("execute");
    let new_sender_nonce = ctx.state.get_nonce(a_id).unwrap();
    assert_eq!(sender_nonce + 1, new_sender_nonce, "nonce should increased");
    let account_id = {
        let mut buf = [0u8; 4];
        buf.copy_from_slice(&return_data);
        u32::from_le_bytes(buf)
    };
    assert_ne!(account_id, 0);

    let script_hash = ctx
        .state
        .get_script_hash(account_id)
        .expect("get script hash");
    assert_ne!(script_hash, H256::zero(), "script hash must exists");
    assert_eq!(
        script_hash,
        contract_script.hash().into(),
        "script hash must according to create account"
    );
}

#[test]
fn test_duplicated_script_hash() {
    init_env_log();
    let mut ctx = TestingContext::setup();

    let a_script = Script::new_builder()
        .code_hash([0u8; 32].pack())
        .args([1u8; 20].to_vec().pack())
        .hash_type(ScriptHashType::Type.into())
        .build();
    let a_script_hash = a_script.hash();
    let a_id = ctx
        .state
        .create_account_from_script(a_script)
        .expect("create account");
    let a_address = ctx.create_eth_address(a_script_hash.into(), [1u8; 20]);

    ctx.state
        .mint_sudt(CKB_SUDT_ACCOUNT_ID, &a_address, U256::from(1000u64))
        .expect("mint CKB for account A to pay fee");

    let block_info = new_block_info(&a_address, 1, 0);

    // create contract
    let contract_script = Script::new_builder()
        .code_hash([0u8; 32].pack())
        .args(vec![42].pack())
        .hash_type(ScriptHashType::Type.into())
        .build();

    let _id = ctx
        .state
        .create_account_from_script(contract_script.clone())
        .expect("create account");

    // should return duplicated script hash
    let args = MetaContractArgs::new_builder()
        .set(
            CreateAccount::new_builder()
                .script(contract_script)
                .fee(
                    Fee::new_builder()
                        .amount(1000u128.pack())
                        .registry_id(a_address.registry_id.pack())
                        .build(),
                )
                .build(),
        )
        .build();
    let result = run_contract_get_result(
        &ctx.rollup_config,
        &mut ctx.state,
        a_id,
        RESERVED_ACCOUNT_ID,
        args.as_bytes(),
        &block_info,
    )
    .unwrap();
    assert_eq!(result.exit_code, GW_ERROR_DUPLICATED_SCRIPT_HASH);
}

#[test]
fn test_insufficient_balance_to_pay_fee() {
    let mut ctx = TestingContext::setup();

    let from_script = Script::new_builder()
        .code_hash([0u8; 32].pack())
        .args([0u8; 20].to_vec().pack())
        .hash_type(ScriptHashType::Type.into())
        .build();
    let from_script_hash = from_script.hash();
    let from_id = ctx
        .state
        .create_account_from_script(from_script)
        .expect("create account");
    let from_address = ctx.create_eth_address(from_script_hash.into(), [1u8; 20]);

    // create contract
    let contract_script = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.pack())
        .hash_type(ScriptHashType::Type.into())
        .args([42u8; 52].pack())
        .build();
    let args = MetaContractArgs::new_builder()
        .set(
            CreateAccount::new_builder()
                .script(contract_script)
                .fee(
                    Fee::new_builder()
                        .amount(1000u128.pack())
                        .registry_id(ctx.eth_registry_id.pack())
                        .build(),
                )
                .build(),
        )
        .build();
    let err = run_contract(
        &ctx.rollup_config,
        &mut ctx.state,
        from_id,
        RESERVED_ACCOUNT_ID,
        args.as_bytes(),
        &new_block_info(&from_address, 1, 0),
    )
    .unwrap_err();
    assert_eq!(err, TransactionError::InsufficientBalance);

    ctx.state
        .mint_sudt(CKB_SUDT_ACCOUNT_ID, &from_address, U256::from(999u64))
        .expect("mint CKB for account A to pay fee");
    let err = run_contract(
        &ctx.rollup_config,
        &mut ctx.state,
        from_id,
        RESERVED_ACCOUNT_ID,
        args.as_bytes(),
        &new_block_info(&from_address, 2, 0),
    )
    .unwrap_err();
    assert_eq!(err, TransactionError::InsufficientBalance);

    ctx.state
        .mint_sudt(CKB_SUDT_ACCOUNT_ID, &from_address, U256::from(1000u64))
        .expect("mint CKB for account A to pay fee");
    let _return_data = run_contract(
        &ctx.rollup_config,
        &mut ctx.state,
        from_id,
        RESERVED_ACCOUNT_ID,
        args.as_bytes(),
        &new_block_info(&from_address, 3, 0),
    )
    .expect("contract created successful");
}
