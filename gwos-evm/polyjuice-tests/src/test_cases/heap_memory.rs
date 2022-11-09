//! Test Recursion Contract
//!   See ./evm-contracts/Memory.sol

use crate::helper::{
    self, deploy, new_block_info, new_contract_account_script, setup, PolyjuiceArgsBuilder,
    CREATOR_ACCOUNT_ID, L2TX_MAX_CYCLES,
};

use gw_common::state::State;

use gw_store::chain_view::ChainView;
use gw_store::traits::chain_store::ChainStore;
use gw_types::{bytes::Bytes, packed::RawL2Transaction, prelude::*};

const MEMORY_INIT_CODE: &str = include_str!("./evm-contracts/Memory.bin");

#[test]
fn test_heap_momory() {
    let (store, mut state, generator) = setup();
    let block_producer_id = helper::create_block_producer(&mut state);

    let from_eth_address = [1u8; 20];
    let (from_id, _from_script_hash) =
        helper::create_eth_eoa_account(&mut state, &from_eth_address, 20000000u64.into());
    let mut block_number = 1;

    // Deploy Memory Contract
    let _run_result = deploy(
        &generator,
        &store,
        &mut state,
        CREATOR_ACCOUNT_ID,
        from_id,
        MEMORY_INIT_CODE,
        122000,
        0,
        block_producer_id.clone(),
        block_number,
    );
    let account_script = new_contract_account_script(&state, from_id, &from_eth_address, false);
    let contract_account_id = state
        .get_account_id_by_script_hash(&account_script.hash().into())
        .unwrap()
        .unwrap();

    {
        // newMemory less than 512K
        let call_code = format!("4e688844{:064x}", 1024 * 15); // < 16 * 32 = 512
        println!("{}", call_code);
        block_number += 1;
        let block_info = new_block_info(block_producer_id.clone(), block_number, block_number);
        let input = hex::decode(call_code).unwrap();
        let args = PolyjuiceArgsBuilder::default()
            .gas_limit(20000000)
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
            .expect("success to malloc memory");
        // [newMemory less than 512K] used cycles: 752,115 -> 883611 (increase 17.48%) < 890K
        helper::check_cycles("new Memory", run_result.cycles, 997_000);
        println!(
            "\t new byte(about {}K) => call result {:?}",
            16 * 32,
            run_result.return_data
        );
    }

    {
        // newMemory more than 512K
        let call_code = format!("4e688844{:064x}", 1024 * 16 + 1);
        println!("{}", call_code);
        block_number += 1;
        let block_info = new_block_info(block_producer_id, block_number, block_number);
        let input = hex::decode(call_code).unwrap();
        let args = PolyjuiceArgsBuilder::default()
            .gas_limit(20000000)
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
        let tip_block_hash = db.get_tip_block_hash().unwrap();
        let err = generator
            .execute_transaction(
                &ChainView::new(&db, tip_block_hash),
                &mut state,
                &block_info,
                &raw_tx,
                L2TX_MAX_CYCLES,
                None,
            )
            .expect_err("OOM");
        println!("{:?}", err);
        // assert_eq!(err, TransactionError::VM(InvalidEcall(64)));
    }

    // for k_bytes in 10..17 {
    //     let call_code = format!("4e688844{:064x}", 1024 * k_bytes);
    //     println!("{}", call_code);
    //     block_number += 1;
    //     let block_info = new_block_info(0, block_number, block_number);
    //     let input = hex::decode(call_code).unwrap();
    //     let args = PolyjuiceArgsBuilder::default()
    //         .gas_limit(20000000)
    //         .gas_price(1)
    //         .value(0)
    //         .input(&input)
    //         .build();
    //     let raw_tx = RawL2Transaction::new_builder()
    //         .from_id(from_id.pack())
    //         .to_id(contract_account_id.pack())
    //         .args(Bytes::from(args).pack())
    //         .build();
    //     let db = store.begin_transaction();
    //     let tip_block_hash = store.get_tip_block_hash().unwrap();
    //     let run_result = generator
    //         .execute_transaction(
    //             &ChainView::new(&db, tip_block_hash),
    //             &state,
    //             &block_info,
    //             &raw_tx,
    //         )
    //         .expect("success to malloc memory");
    //     println!(
    //         "\t new byte({}K) => call result {:?}",
    //         k_bytes, run_result.return_data
    //     );
    // }
}
