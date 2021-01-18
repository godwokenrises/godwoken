use super::new_block_info;
use crate::{account_lock_manage::AccountLockManage, Generator};
use crate::{backend_manage::BackendManage, dummy_state::DummyState};
use crate::{builtin_scripts::SUDT_VALIDATOR_CODE_HASH, error::TransactionError, traits::StateExt};
use core::panic;
use gw_common::state::State;
use gw_common::{h256_ext::H256Ext, H256};
use gw_store::{CodeStore, Store};
use gw_types::{
    packed::{BlockInfo, RawL2Transaction, SUDTArgs, SUDTQuery, SUDTTransfer, Script},
    prelude::*,
};

const ERROR_INSUFFICIENT_BALANCE: i8 = 12i8;

fn run_contract<S: State + CodeStore>(
    store: &Store,
    tree: &mut S,
    from_id: u32,
    to_id: u32,
    args: SUDTArgs,
    block_info: &BlockInfo,
) -> Result<Vec<u8>, TransactionError> {
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(to_id.pack())
        .args(args.as_bytes().pack())
        .build();
    let backend_manage = BackendManage::default();
    let account_lock_manage = AccountLockManage::default();
    let generator = Generator::new(backend_manage, account_lock_manage, Default::default());
    let run_result = generator.execute(&store.begin_transaction(), tree, block_info, &raw_tx)?;
    tree.apply_run_result(&run_result).expect("update state");
    Ok(run_result.return_data)
}

#[test]
fn test_sudt() {
    let store = Store::open_tmp().unwrap();
    let mut tree = DummyState::default();
    let init_a_balance: u128 = 10000;

    // init accounts
    let sudt_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash(Into::<[u8; 32]>::into(SUDT_VALIDATOR_CODE_HASH.clone()).pack())
                .args([0u8; 32].to_vec().pack())
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
    let aggregator_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([0u8; 20].to_vec().pack())
                .build(),
        )
        .expect("create account");
    let block_info = new_block_info(aggregator_id, 1, 0);

    // init balance for a
    tree.update_value(
        sudt_id,
        &H256::from_u32(a_id),
        H256::from_u128(init_a_balance).into(),
    )
    .expect("init balance");

    // check balance of A, B
    {
        let args = SUDTArgs::new_builder()
            .set(SUDTQuery::new_builder().account_id(a_id.pack()).build())
            .build();
        let return_data =
            run_contract(&store, &mut tree, a_id, sudt_id, args, &block_info).expect("execute");
        let balance = {
            let mut buf = [0u8; 16];
            buf.copy_from_slice(&return_data);
            u128::from_le_bytes(buf)
        };
        assert_eq!(balance, init_a_balance);

        let args = SUDTArgs::new_builder()
            .set(SUDTQuery::new_builder().account_id(b_id.pack()).build())
            .build();
        let return_data =
            run_contract(&store, &mut tree, a_id, sudt_id, args, &block_info).expect("execute");
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
        let fee = 42u128;
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to(b_id.pack())
                    .amount(value.pack())
                    .fee(fee.pack())
                    .build(),
            )
            .build();
        let return_data =
            run_contract(&store, &mut tree, a_id, sudt_id, args, &block_info).expect("execute");
        assert!(return_data.is_empty());

        {
            let args = SUDTArgs::new_builder()
                .set(SUDTQuery::new_builder().account_id(a_id.pack()).build())
                .build();
            let return_data =
                run_contract(&store, &mut tree, a_id, sudt_id, args, &block_info).expect("execute");
            let balance = {
                let mut buf = [0u8; 16];
                buf.copy_from_slice(&return_data);
                u128::from_le_bytes(buf)
            };
            assert_eq!(balance, init_a_balance - value - fee);

            let args = SUDTArgs::new_builder()
                .set(SUDTQuery::new_builder().account_id(b_id.pack()).build())
                .build();
            let return_data =
                run_contract(&store, &mut tree, a_id, sudt_id, args, &block_info).expect("execute");
            let balance = {
                let mut buf = [0u8; 16];
                buf.copy_from_slice(&return_data);
                u128::from_le_bytes(buf)
            };
            assert_eq!(balance, value);

            let args = SUDTArgs::new_builder()
                .set(
                    SUDTQuery::new_builder()
                        .account_id(aggregator_id.pack())
                        .build(),
                )
                .build();
            let return_data =
                run_contract(&store, &mut tree, a_id, sudt_id, args, &block_info).expect("execute");
            let balance = {
                let mut buf = [0u8; 16];
                buf.copy_from_slice(&return_data);
                u128::from_le_bytes(buf)
            };
            assert_eq!(balance, fee);
        }
    }
}

#[test]
fn test_insufficient_balance() {
    let store = Store::open_tmp().unwrap();
    let mut tree = DummyState::default();
    let init_a_balance: u128 = 10000;

    // init accounts
    let sudt_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash(Into::<[u8; 32]>::into(SUDT_VALIDATOR_CODE_HASH.clone()).pack())
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

    let block_info = new_block_info(0, 10, 0);

    // init balance for a
    tree.update_value(
        sudt_id,
        &H256::from_u32(a_id),
        H256::from_u128(init_a_balance),
    )
    .expect("update init balance");

    // transfer from A to B
    {
        let value = 10001u128;
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to(b_id.pack())
                    .amount(value.pack())
                    .build(),
            )
            .build();
        let err =
            run_contract(&store, &mut tree, a_id, sudt_id, args, &block_info).expect_err("err");
        let err_code = match err {
            TransactionError::InvalidExitCode(code) => code,
            err => panic!("unexpected {:?}", err),
        };
        assert_eq!(err_code, ERROR_INSUFFICIENT_BALANCE);
    }
}
