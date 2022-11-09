//! Test Recursion Contract
//!   See ./evm-contracts/RecursionContract.sol

use crate::helper::{
    self, deploy, new_block_info, new_contract_account_script, setup, PolyjuiceArgsBuilder,
    CREATOR_ACCOUNT_ID, L2TX_MAX_CYCLES,
};
use gw_common::state::State;
use gw_generator::error::TransactionError;
use gw_store::traits::chain_store::ChainStore;
use gw_store::{chain_view::ChainView, state::traits::JournalDB};
use gw_types::{bytes::Bytes, packed::RawL2Transaction, prelude::*};

const RECURSION_INIT_CODE: &str = include_str!("./evm-contracts/RecursionContract.bin");

#[test]
fn test_recursion_contract_call() {
    let (store, mut state, generator) = setup();
    let block_producer_id = helper::create_block_producer(&mut state);

    let from_eth_address = [1u8; 20];
    let (from_id, _from_script_hash) =
        helper::create_eth_eoa_account(&mut state, &from_eth_address, 200000u64.into());
    let mut block_number = 1;

    // Deploy RecursionContract
    let _run_result = deploy(
        &generator,
        &store,
        &mut state,
        CREATOR_ACCOUNT_ID,
        from_id,
        RECURSION_INIT_CODE,
        122000,
        0,
        block_producer_id.clone(),
        block_number,
    );
    block_number += 1;
    let recur_account_script =
        new_contract_account_script(&state, from_id, &from_eth_address, false);
    let recur_account_id = state
        .get_account_id_by_script_hash(&recur_account_script.hash().into())
        .unwrap()
        .unwrap();

    {
        // Call Sum(31), 31 < max_depth=32
        let block_info = new_block_info(block_producer_id.clone(), block_number, block_number);
        let input =
            hex::decode("188b85b4000000000000000000000000000000000000000000000000000000000000001f")
                .unwrap();
        let args = PolyjuiceArgsBuilder::default()
            .gas_limit(200000)
            .gas_price(1)
            .value(0)
            .input(&input)
            .build();
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(recur_account_id.pack())
            .args(Bytes::from(args).pack())
            .build();
        let db = &store.begin_transaction();
        let tip_block_hash = store.get_tip_block_hash().unwrap();
        let run_result = generator
            .execute_transaction(
                &ChainView::new(&db, tip_block_hash),
                &mut state,
                &block_info,
                &raw_tx,
                L2TX_MAX_CYCLES,
                None,
            )
            .expect("recursive call depth to 32");
        state.finalise().expect("update state");
        println!("\t call result {:?}", run_result.return_data);
        let expected_sum = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 1, 240,
        ];
        assert_eq!(run_result.return_data.as_ref(), expected_sum);
    }

    // {
    //     // EVMC_CALL_DEPTH_EXCEEDED Case
    //     block_number += 1;
    //     let block_info = new_block_info(0, block_number, block_number);
    //     let input =
    //         hex::decode("188b85b40000000000000000000000000000000000000000000000000000000000000020")
    //             .unwrap();
    //     let args = PolyjuiceArgsBuilder::default()
    //         .gas_limit(200000)
    //         .gas_price(1)
    //         .value(0)
    //         .input(&input)
    //         .build();
    //     let raw_tx = RawL2Transaction::new_builder()
    //         .from_id(from_id.pack())
    //         .to_id(recur_account_id.pack())
    //         .args(Bytes::from(args).pack())
    //         .build();
    //     let db = store.begin_transaction();
    //     let tip_block_hash = store.get_tip_block_hash().unwrap();
    //     let err = generator
    //         .execute_transaction(
    //             &ChainView::new(&db, tip_block_hash),
    //             &state,
    //             &block_info,
    //             &raw_tx,
    //         )
    //         .expect_err("EVMC_CALL_DEPTH_EXCEEDED = -52");
    //     assert_eq!(err, TransactionError::InvalidExitCode(-52));
    // }

    {
        // Case: out of gas and revert
        block_number += 1;
        let block_info = new_block_info(block_producer_id.clone(), block_number, block_number);
        let input =
            hex::decode("188b85b40000000000000000000000000000000000000000000000000000000000000020")
                .unwrap();
        let args = PolyjuiceArgsBuilder::default()
            .gas_limit(50000)
            .gas_price(1)
            .value(0)
            .input(&input)
            .build();
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(recur_account_id.pack())
            .args(Bytes::from(args).pack())
            .build();
        let db = &store.begin_transaction();
        let tip_block_hash = store.get_tip_block_hash().unwrap();
        let err = generator
            .execute_transaction(
                &ChainView::new(&db, tip_block_hash),
                &mut state,
                &block_info,
                &raw_tx,
                L2TX_MAX_CYCLES,
                None,
            )
            .unwrap();
        assert_eq!(err.exit_code, 2);
    }

    {
        // Case: out of gas and no revert
        block_number += 1;
        let block_info = new_block_info(block_producer_id, block_number, block_number);
        let input =
            hex::decode("188b85b40000000000000000000000000000000000000000000000000000000000000020")
                .unwrap();
        let args = PolyjuiceArgsBuilder::default()
            .gas_limit(4100)
            .gas_price(1)
            .value(0)
            .input(&input)
            .build();
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(recur_account_id.pack())
            .args(Bytes::from(args).pack())
            .build();
        let db = &store.begin_transaction();
        let tip_block_hash = db.get_tip_block_hash().unwrap();
        let err = generator.execute_transaction(
            &ChainView::new(&db, tip_block_hash),
            &mut state,
            &block_info,
            &raw_tx,
            L2TX_MAX_CYCLES,
            None,
        );
        assert_eq!(err.unwrap_err(), TransactionError::InsufficientBalance);
    }
}
