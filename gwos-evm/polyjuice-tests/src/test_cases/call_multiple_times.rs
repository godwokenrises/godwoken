//! Test contract call contract multiple times
//!   See ./evm-contracts/CallContract.sol

use crate::helper::{
    create_eth_eoa_account, deploy, new_block_info, setup, simple_storage_get, MockContractInfo,
    PolyjuiceArgsBuilder, CREATOR_ACCOUNT_ID, L2TX_MAX_CYCLES,
};
use gw_common::state::State;
use gw_store::traits::chain_store::ChainStore;
use gw_store::{chain_view::ChainView, state::traits::JournalDB};
use gw_types::{bytes::Bytes, packed::RawL2Transaction, prelude::*};

const SS_INIT_CODE: &str = include_str!("./evm-contracts/SimpleStorage.bin");
const INIT_CODE: &str = include_str!("./evm-contracts/CallMultipleTimes.bin");

#[test]
fn test_call_multiple_times() {
    let (store, mut state, generator) = setup();
    let block_producer_id = crate::helper::create_block_producer(&mut state);

    let from_eth_address = [1u8; 20];
    let (from_id, _) = create_eth_eoa_account(&mut state, &from_eth_address, 500000u64.into());

    // Deploy two SimpleStorage
    let mut block_number = 1;
    for _ in 0..2 {
        let _run_result = deploy(
            &generator,
            &store,
            &mut state,
            CREATOR_ACCOUNT_ID,
            from_id,
            SS_INIT_CODE,
            122000,
            0,
            block_producer_id.clone(),
            block_number,
        );
        state.finalise().expect("update state");
        block_number += 1;
    }

    let ss1_contract = MockContractInfo::create(&from_eth_address, 0);
    let ss1_contract_eth_abi_addr = ss1_contract.eth_abi_addr;
    let ss1_contract_script_hash = ss1_contract.script_hash;

    let ss1_account_id = state
        .get_account_id_by_script_hash(&ss1_contract_script_hash)
        .unwrap()
        .unwrap();
    let ss2_contract = MockContractInfo::create(&from_eth_address, 1);
    let ss2_contract_eth_abi_addr = ss2_contract.eth_abi_addr;
    let ss2_contract_script_hash = ss2_contract.script_hash;

    let ss2_account_id = state
        .get_account_id_by_script_hash(&ss2_contract_script_hash)
        .unwrap()
        .unwrap();

    // Deploy CallMultipleTimes - constructor(address)
    let input = format!("{}{}", INIT_CODE, hex::encode(ss1_contract_eth_abi_addr));
    let _run_result = deploy(
        &generator,
        &store,
        &mut state,
        CREATOR_ACCOUNT_ID,
        from_id,
        input.as_str(),
        122000,
        0,
        block_producer_id.clone(),
        block_number,
    );

    // state.apply_run_result(&_run_result).expect("update state");
    block_number += 1;
    // println!(
    //     "result {}",
    //     serde_json::to_string_pretty(&RunResult::from(run_result)).unwrap()
    // );
    let cm_contract = MockContractInfo::create(&from_eth_address, 2);

    let cm_contract_id = state
        .get_account_id_by_script_hash(&cm_contract.script_hash)
        .unwrap()
        .unwrap();

    let run_result = simple_storage_get(
        &store,
        &mut state,
        &generator,
        block_number,
        from_id,
        ss1_account_id,
    );
    assert_eq!(
        run_result.return_data,
        hex::decode("000000000000000000000000000000000000000000000000000000000000007b").unwrap()
    );
    let run_result = simple_storage_get(
        &store,
        &mut state,
        &generator,
        block_number,
        from_id,
        ss2_account_id,
    );
    assert_eq!(
        run_result.return_data,
        hex::decode("000000000000000000000000000000000000000000000000000000000000007b").unwrap()
    );

    assert_eq!(state.get_nonce(ss1_account_id).unwrap(), 0);
    assert_eq!(state.get_nonce(ss2_account_id).unwrap(), 0);
    assert_eq!(state.get_nonce(cm_contract_id).unwrap(), 0);

    {
        // CallMultipleTimes.proxySet(20);
        let block_info = new_block_info(block_producer_id, block_number, block_number);
        // let ss2_contract_ethabi_addr = contract_script_to_eth_addr(&ss2_account_script, true);
        let input = hex::decode(format!(
            "bca0b9c2{}{}",
            hex::encode(&ss2_contract_eth_abi_addr),
            "0000000000000000000000000000000000000000000000000000000000000014",
        ))
        .unwrap();
        let args = PolyjuiceArgsBuilder::default()
            .gas_limit(163263)
            .gas_price(1)
            .value(0)
            .input(&input)
            .build();
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(cm_contract_id.pack())
            .args(Bytes::from(args).pack())
            .build();
        let db = &store.begin_transaction();
        let tip_block_hash = db.get_tip_block_hash().unwrap();
        let _run_result = generator
            .execute_transaction(
                &ChainView::new(&db, tip_block_hash),
                &mut state,
                &block_info,
                &raw_tx,
                L2TX_MAX_CYCLES,
                None,
            )
            .expect("CallMultipleTimes.proxySet(20)");
        state.finalise().expect("update state");
        // println!(
        //     "result {}",
        //     serde_json::to_string_pretty(&RunResult::from(run_result)).unwrap()
        // );
    }

    assert_eq!(state.get_nonce(ss1_account_id).unwrap(), 0);
    assert_eq!(state.get_nonce(ss2_account_id).unwrap(), 0);
    assert_eq!(state.get_nonce(cm_contract_id).unwrap(), 0);

    let run_result = simple_storage_get(
        &store,
        &mut state,
        &generator,
        block_number,
        from_id,
        ss1_account_id,
    );
    assert_eq!(
        run_result.return_data,
        hex::decode("0000000000000000000000000000000000000000000000000000000000000016").unwrap()
    );
    let run_result = simple_storage_get(
        &store,
        &mut state,
        &generator,
        block_number,
        from_id,
        ss2_account_id,
    );
    assert_eq!(
        run_result.return_data,
        hex::decode("0000000000000000000000000000000000000000000000000000000000000019").unwrap()
    );
}
