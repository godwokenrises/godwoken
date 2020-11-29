use super::{build_dummy_state, new_block_info, SUDT_PROGRAM_CODE_HASH};
use crate::dummy_state::DummyState;
use crate::traits::{CodeStore, StateExt};
use crate::Error;
use crate::Generator;
use gw_common::blake2b::new_blake2b;
use gw_common::state::State;
use gw_types::{
    core::CallType,
    packed::{CallContext, SUDTArgs, SUDTQuery, SUDTTransfer, Script},
    prelude::*,
};
use std::mem::size_of_val;

const ERROR_INSUFFICIENT_BALANCE: i8 = 12i8;

fn build_sudt_key(token_id: &[u8], account_id: u32) -> [u8; 32] {
    let mut hasher = new_blake2b();
    hasher.update(&token_id);
    hasher.update(&account_id.to_le_bytes());
    let mut buf = [0u8; 32];
    hasher.finalize(&mut buf);
    buf
}

fn run_contract<S: State + CodeStore>(
    tree: &mut S,
    from_id: u32,
    to_id: u32,
    args: SUDTArgs,
) -> Result<Vec<u8>, Error> {
    let block_info = new_block_info(0, 1, 0);
    let call_context = CallContext::new_builder()
        .from_id(from_id.pack())
        .to_id(to_id.pack())
        .call_type(CallType::HandleMessage.into())
        .args(args.as_bytes().pack())
        .build();
    let generator = Generator::default();
    let run_result = generator.execute(tree, &block_info, &call_context)?;
    tree.apply_run_result(&run_result).expect("update state");
    Ok(run_result.return_data)
}

#[test]
fn test_sudt() {
    let mut tree = build_dummy_state();
    let init_a_balance: u128 = 10000;
    let token_id = [0u8; 32];

    // init accounts
    let contract_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash(SUDT_PROGRAM_CODE_HASH.pack())
                .args([0u8; 20].to_vec().pack())
                .build(),
        )
        .expect("create account");
    let a_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([0u8; 20].to_vec().pack())
                .build(),
        )
        .expect("create account");
    let b_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([0u8; 20].to_vec().pack())
                .build(),
        )
        .expect("create account");

    // run constructor (do nothing)
    {
        let block_info = new_block_info(0, 0, 0);
        let call_context = CallContext::new_builder()
            .from_id(a_id.pack())
            .to_id(contract_id.pack())
            .call_type(CallType::Construct.into())
            .build();
        let generator = Generator::default();
        let run_result = generator
            .execute(&tree, &block_info, &call_context)
            .expect("construct");
        tree.apply_run_result(&run_result).expect("update state");
    }

    // init balance for a
    let mut value = [0u8; 32];
    value[..size_of_val(&init_a_balance)].copy_from_slice(&init_a_balance.to_le_bytes());
    let a_state_key = build_sudt_key(&token_id, a_id);
    println!("a_state_key = {:?}", a_state_key);
    tree.update_value(contract_id, &a_state_key, value.into())
        .expect("update init balance");

    // check balance of A, B
    {
        let args = SUDTArgs::new_builder()
            .set(
                SUDTQuery::new_builder()
                    .token_id(token_id.pack())
                    .account_id(a_id.pack())
                    .build(),
            )
            .build();
        let return_data = run_contract(&mut tree, a_id, contract_id, args).expect("execute");
        let balance = {
            let mut buf = [0u8; 16];
            buf.copy_from_slice(&return_data);
            u128::from_le_bytes(buf)
        };
        assert_eq!(balance, init_a_balance);

        let args = SUDTArgs::new_builder()
            .set(
                SUDTQuery::new_builder()
                    .token_id(token_id.pack())
                    .account_id(b_id.pack())
                    .build(),
            )
            .build();
        let return_data = run_contract(&mut tree, a_id, contract_id, args).expect("execute");
        let balance = {
            let mut buf = [0u8; 16];
            buf.copy_from_slice(&return_data);
            u128::from_le_bytes(buf)
        };
        assert_eq!(balance, 0);
    }

    // transfer from A to B
    {
        let value = 4000u128;
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .token_id(token_id.pack())
                    .to(b_id.pack())
                    .value(value.pack())
                    .build(),
            )
            .build();
        let return_data = run_contract(&mut tree, a_id, contract_id, args).expect("execute");
        assert!(return_data.is_empty());

        {
            let args = SUDTArgs::new_builder()
                .set(
                    SUDTQuery::new_builder()
                        .token_id(token_id.pack())
                        .account_id(a_id.pack())
                        .build(),
                )
                .build();
            let return_data = run_contract(&mut tree, a_id, contract_id, args).expect("execute");
            let balance = {
                let mut buf = [0u8; 16];
                buf.copy_from_slice(&return_data);
                u128::from_le_bytes(buf)
            };
            assert_eq!(balance, init_a_balance - value);

            let args = SUDTArgs::new_builder()
                .set(
                    SUDTQuery::new_builder()
                        .token_id(token_id.pack())
                        .account_id(b_id.pack())
                        .build(),
                )
                .build();
            let return_data = run_contract(&mut tree, a_id, contract_id, args).expect("execute");
            let balance = {
                let mut buf = [0u8; 16];
                buf.copy_from_slice(&return_data);
                u128::from_le_bytes(buf)
            };
            assert_eq!(balance, value);
        }
    }
}

#[test]
fn test_sudt_insufficient_balance() {
    let mut tree = build_dummy_state();
    let init_a_balance: u128 = 10000;
    let token_id = [0u8; 32];

    // init accounts
    let contract_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash(SUDT_PROGRAM_CODE_HASH.pack())
                .args([0u8; 20].to_vec().pack())
                .build(),
        )
        .expect("create account");
    let a_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([0u8; 20].to_vec().pack())
                .build(),
        )
        .expect("create account");
    let b_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([0u8; 20].to_vec().pack())
                .build(),
        )
        .expect("create account");

    // run constructor (do nothing)
    {
        let block_info = new_block_info(0, 0, 0);
        let call_context = CallContext::new_builder()
            .from_id(a_id.pack())
            .to_id(contract_id.pack())
            .call_type(CallType::Construct.into())
            .build();
        let generator = Generator::default();
        let run_result = generator
            .execute(&tree, &block_info, &call_context)
            .expect("construct");
        tree.apply_run_result(&run_result).expect("update state");
    }

    // init balance for a
    let mut value = [0u8; 32];
    value[..size_of_val(&init_a_balance)].copy_from_slice(&init_a_balance.to_le_bytes());
    let a_state_key = build_sudt_key(&token_id, a_id);
    tree.update_value(contract_id, &a_state_key, value.into())
        .expect("update init balance");

    // transfer from A to B
    {
        let value = 10001u128;
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .token_id(token_id.pack())
                    .to(b_id.pack())
                    .value(value.pack())
                    .build(),
            )
            .build();
        let err = run_contract(&mut tree, a_id, contract_id, args).expect_err("err");
        let err_code = match err {
            Error::InvalidExitCode(code) => code,
            err => panic!("unexpected {:?}", err),
        };
        assert_eq!(err_code, ERROR_INSUFFICIENT_BALANCE);
    }
}
