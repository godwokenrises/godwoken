use gw_common::state::State;
use gw_store::{chain_view::ChainView, traits::chain_store::ChainStore};
use gw_types::{
    bytes::Bytes,
    packed::RawL2Transaction,
    prelude::{Builder, Entity, Pack},
};

use crate::helper::{self, L2TX_MAX_CYCLES};

const CONTRACT_CODE: &str = include_str!("./evm-contracts/Error.bin");

#[test]
fn test_error_handling() {
    let (store, mut state, generator) = helper::setup();
    let block_producer = helper::create_block_producer(&mut state);

    // init accounts
    let from_eth_address = [1u8; 20];
    let (from_id, _from_script_hash) =
        helper::create_eth_eoa_account(&mut state, &from_eth_address, 400000u64.into());

    // deploy Error contract
    let run_result = helper::deploy(
        &generator,
        &store,
        &mut state,
        helper::CREATOR_ACCOUNT_ID,
        from_id,
        CONTRACT_CODE,
        233474,
        0,
        block_producer.to_owned(),
        4,
    );
    assert_eq!(run_result.exit_code, crate::constant::EVMC_SUCCESS);
    let contract_script =
        helper::new_contract_account_script(&state, from_id, &from_eth_address, false);
    let contract_account_id = state
        .get_account_id_by_script_hash(&contract_script.hash().into())
        .unwrap()
        .expect("get_account_id_by_script_hash");

    let mut block_number = 0;

    // Call testAssert() -> 0x2b813bc0
    block_number += 1;
    let block_info = helper::new_block_info(block_producer.to_owned(), block_number, block_number);
    let input = hex::decode("2b813bc0").expect("testAssert() method ID");
    let args = helper::PolyjuiceArgsBuilder::default()
        .gas_limit(30000)
        .gas_price(1)
        .value(0)
        .input(&input)
        .build();
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(contract_account_id.pack())
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
        .expect("Call testAssert()");
    assert_eq!(run_result.exit_code, crate::constant::EVMC_SUCCESS);

    // Panic via assert: Call mockAssertPayable() -> 0x67478cc9
    block_number += 1;
    let block_info = helper::new_block_info(block_producer.to_owned(), block_number, block_number);
    let input = hex::decode("67478cc9").expect("mockAssertPayable() method ID");
    let args = helper::PolyjuiceArgsBuilder::default()
        .gas_limit(30001)
        .gas_price(1)
        .value(0)
        .input(&input)
        .build();
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(contract_account_id.pack())
        .args(Bytes::from(args).pack())
        .build();
    let db = &store.begin_transaction();
    let tip_block_hash = store.get_tip_block_hash().unwrap();
    let run_result = generator
        .unchecked_execute_transaction(
            &ChainView::new(&db, tip_block_hash),
            &mut state,
            &block_info,
            &raw_tx,
            L2TX_MAX_CYCLES,
            None,
        )
        .expect("mockAssertPayable() => EVMC_REVERT: 2");
    assert_eq!(run_result.exit_code, crate::constant::EVMC_REVERT);
    // The assert function creates an built-in error of type Panic(uint256) -> 0x4e487b71
    // 0x01: If you call assert with an argument that evaluates to false.
    // evmc_result.output_size => 36
    // evmc_result.output_data: 0x4e487b710000000000000000000000000000000000000000000000000000000000000001
    let expected_output =
        hex::decode("4e487b710000000000000000000000000000000000000000000000000000000000000001")
            .expect("decode Panic(1)");
    assert_eq!(run_result.return_data, expected_output);

    // Call testOverflowError(1) -> 0x8a8a9b640000000000000000000000000000000000000000000000000000000000000001
    block_number += 1;
    let block_info = helper::new_block_info(block_producer.to_owned(), block_number, block_number);
    let input =
        hex::decode("8a8a9b640000000000000000000000000000000000000000000000000000000000000001")
            .expect("decode the function selector and arg of testOverflowError(1)");
    let args = helper::PolyjuiceArgsBuilder::default()
        .gas_limit(30002)
        .gas_price(1)
        .value(0)
        .input(&input)
        .build();
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(contract_account_id.pack())
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
    // The assert function creates an built-in error of type Panic(uint256) -> 0x4e487b71
    // 0x11: If an arithmetic operation results in underflow or overflow outside of an unchecked { ... } block.
    // evmc_result.output_size => 36
    // evmc_result.output_data: 0x4e487b710000000000000000000000000000000000000000000000000000000000000011
    assert_eq!(err.exit_code, crate::constant::EVMC_REVERT);

    // Call testRequire(9) -> 0xb8bd717f0000000000000000000000000000000000000000000000000000000000000009
    block_number += 1;
    let block_info = helper::new_block_info(block_producer.to_owned(), block_number, block_number);
    let input =
        hex::decode("b8bd717f0000000000000000000000000000000000000000000000000000000000000009")
            .expect("decode the function selector and arg of testRequire(9)");
    let args = helper::PolyjuiceArgsBuilder::default()
        .gas_limit(30003)
        .gas_price(1)
        .value(0)
        .input(&input)
        .build();
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(contract_account_id.pack())
        .args(Bytes::from(args).pack())
        .build();
    let db = &store.begin_transaction();
    let tip_block_hash = store.get_tip_block_hash().unwrap();
    let run_result = generator
        .unchecked_execute_transaction(
            &ChainView::new(&db, tip_block_hash),
            &mut state,
            &block_info,
            &raw_tx,
            L2TX_MAX_CYCLES,
            None,
        )
        .expect("testRequire(9) => EVMC_REVERT: 2");
    assert_eq!(run_result.exit_code, crate::constant::EVMC_REVERT);
    // The require function creates an built-in error of type Error(string) -> 0x08c379a0
    // evmc_result.output_size => 100
    // evmc_result.output_data: 0x08c379a00000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000001d496e707574206d7573742062652067726561746572207468616e203130000000
    let expected_output = hex::decode("08c379a00000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000001d496e707574206d7573742062652067726561746572207468616e203130000000")
        .expect("decode Error('Input must be greater than 10')");
    assert_eq!(run_result.return_data, expected_output);

    // Call testRevert(8) -> 0x209877670000000000000000000000000000000000000000000000000000000000000008
    block_number += 1;
    let block_info = helper::new_block_info(block_producer.to_owned(), block_number, block_number);
    let input =
        hex::decode("209877670000000000000000000000000000000000000000000000000000000000000008")
            .expect("decode the function selector and arg of testRevert(8)");
    let args = helper::PolyjuiceArgsBuilder::default()
        .gas_limit(30004)
        .gas_price(1)
        .value(0)
        .input(&input)
        .build();
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(contract_account_id.pack())
        .args(Bytes::from(args).pack())
        .build();
    let db = &store.begin_transaction();
    let tip_block_hash = store.get_tip_block_hash().unwrap();
    let run_result = generator
        .unchecked_execute_transaction(
            &ChainView::new(&db, tip_block_hash),
            &mut state,
            &block_info,
            &raw_tx,
            L2TX_MAX_CYCLES,
            None,
        )
        .expect("Call testRevert(8)");
    assert_eq!(run_result.exit_code, crate::constant::EVMC_REVERT);
    // The revert function creates an built-in error of type Error(string) -> 0x08c379a0
    // evmc_result.output_size => 100
    // evmc_result.output_data: 0x08c379a00000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000001d496e707574206d7573742062652067726561746572207468616e203130000000
    assert_eq!(run_result.return_data, expected_output);

    // testRevertMsg("test revert message") -> 0x5729f42c000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000000137465737420726576657274206d65737361676500000000000000000000000000
    block_number += 1;
    let block_info = helper::new_block_info(block_producer, block_number, block_number);
    let input =
        hex::decode("5729f42c000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000000137465737420726576657274206d65737361676500000000000000000000000000")
            .expect("decode the function selector and arg of testRevertMsg('test revert message')");
    let args = helper::PolyjuiceArgsBuilder::default()
        .gas_limit(30005)
        .gas_price(1)
        .value(0)
        .input(&input)
        .build();
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(contract_account_id.pack())
        .args(Bytes::from(args).pack())
        .build();
    let db = &store.begin_transaction();
    let tip_block_hash = store.get_tip_block_hash().unwrap();
    let run_result = generator
        .unchecked_execute_transaction(
            &ChainView::new(&db, tip_block_hash),
            &mut state,
            &block_info,
            &raw_tx,
            L2TX_MAX_CYCLES,
            None,
        )
        .expect("Call testRevertMsg('test revert message')");
    assert_eq!(run_result.exit_code, crate::constant::EVMC_REVERT);
    let expected_output = hex::decode("08c379a0000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000000137465737420726576657274206d65737361676500000000000000000000000000")
        .expect("decode Error('test revert message')");
    assert_eq!(run_result.return_data, expected_output);
}
