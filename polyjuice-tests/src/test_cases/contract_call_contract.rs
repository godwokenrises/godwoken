//! Test contract call contract
//!   See ./evm-contracts/CallContract.sol

use crate::helper::{
    self, create_eth_eoa_account, deploy, eth_addr_to_ethabi_addr, new_block_info, setup,
    simple_storage_get, MockContractInfo, PolyjuiceArgsBuilder, CREATOR_ACCOUNT_ID,
    L2TX_MAX_CYCLES,
};
use gw_common::state::State;
use gw_store::traits::chain_store::ChainStore;
use gw_store::{chain_view::ChainView, state::traits::JournalDB};
use gw_types::{bytes::Bytes, packed::RawL2Transaction, prelude::*};

const SS_INIT_CODE: &str = include_str!("./evm-contracts/SimpleStorage.bin");
const INIT_CODE: &str = include_str!("./evm-contracts/CallContract.bin");
const CALL_NON_EXISTS_INIT_CODE: &str = include_str!("./evm-contracts/CallNonExistsContract.bin");

#[test]
fn test_contract_call_contract() {
    let (store, mut state, generator) = setup();
    let block_producer = helper::create_block_producer(&mut state);

    let from_eth_address = [1u8; 20];
    let (from_id, _from_script_hash) =
        helper::create_eth_eoa_account(&mut state, &from_eth_address, 400000u64.into());

    // Deploy SimpleStorage
    let mut block_number = 1;
    let _run_result = deploy(
        &generator,
        &store,
        &mut state,
        CREATOR_ACCOUNT_ID,
        from_id,
        SS_INIT_CODE,
        77659,
        0,
        block_producer.clone(),
        block_number,
    );

    let ss_account = MockContractInfo::create(&from_eth_address, 0);
    let ss_eth_abi_addr = ss_account.eth_abi_addr;
    let ss_script_hash = ss_account.script_hash;
    let ss_account_id = state
        .get_account_id_by_script_hash(&ss_script_hash)
        .unwrap()
        .unwrap();

    // Deploy CallContract
    block_number += 1;
    let input = format!("{}{}", INIT_CODE, hex::encode(&ss_eth_abi_addr));
    let run_result = deploy(
        &generator,
        &store,
        &mut state,
        CREATOR_ACCOUNT_ID,
        from_id,
        input.as_str(),
        84209,
        0,
        block_producer.clone(),
        block_number,
    );
    // println!(
    //     "result {}",
    //     serde_json::to_string_pretty(&RunResult::from(run_result)).unwrap()
    // );
    // [Deploy CreateContract] used cycles: 600288 < 610K
    helper::check_cycles("Deploy CreateContract", run_result.cycles, 880_000);
    let cc_account = MockContractInfo::create(&from_eth_address, 1);
    let cc_contract_id = state
        .get_account_id_by_script_hash(&cc_account.script_hash)
        .unwrap()
        .unwrap();

    block_number += 1;
    let run_result = simple_storage_get(
        &store,
        &mut state,
        &generator,
        block_number,
        from_id,
        ss_account_id,
    );
    assert_eq!(
        run_result.return_data, // default storedData = 123
        hex::decode("000000000000000000000000000000000000000000000000000000000000007b").unwrap()
    );

    {
        // CallContract.proxySet(222) => SimpleStorage.set(x+3)
        block_number += 1;
        let block_info = new_block_info(block_producer, block_number, block_number);
        let input =
            hex::decode("28cc7b2500000000000000000000000000000000000000000000000000000000000000de")
                .unwrap(); // 0xde = 222
        let args = PolyjuiceArgsBuilder::default()
            .gas_limit(71000)
            .gas_price(1)
            .value(0)
            .input(&input)
            .build();
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(cc_contract_id.pack())
            .args(Bytes::from(args).pack())
            .build();
        let db = &store.begin_transaction();
        let tip_block_hash = db.get_tip_block_hash().unwrap();
        let run_result = generator
            .execute_transaction(
                &ChainView::new(&db, tip_block_hash),
                &mut state,
                &block_info,
                &raw_tx,
                L2TX_MAX_CYCLES,
                None,
            )
            .expect("CallContract.proxySet");
        state.finalise().expect("update state");
        // [CallContract.proxySet(222)] used cycles: 961599 -> 980564 < 981K
        helper::check_cycles("CallContract.proxySet()", run_result.cycles, 1_170_000);
    }

    let run_result = simple_storage_get(
        &store,
        &mut state,
        &generator,
        block_number,
        from_id,
        ss_account_id,
    );
    assert_eq!(
        run_result.return_data, // 0xe1 = 225
        hex::decode("00000000000000000000000000000000000000000000000000000000000000e1").unwrap()
    );

    assert_eq!(state.get_nonce(from_id).unwrap(), 5);
    assert_eq!(state.get_nonce(ss_account_id).unwrap(), 0);
    assert_eq!(state.get_nonce(cc_contract_id).unwrap(), 0);
}

#[test]
fn test_contract_call_non_exists_contract() {
    let (store, mut state, generator) = setup();
    let block_producer_id = helper::create_block_producer(&mut state);

    let from_eth_address = [1u8; 20];
    let (from_id, _from_script_hash) =
        helper::create_eth_eoa_account(&mut state, &from_eth_address, 200000u64.into());

    // Deploy CallNonExistsContract
    let block_number = 1;
    let run_result = deploy(
        &generator,
        &store,
        &mut state,
        CREATOR_ACCOUNT_ID,
        from_id,
        CALL_NON_EXISTS_INIT_CODE,
        1220000,
        0,
        block_producer_id.clone(),
        block_number,
    );
    // [Deploy CallNonExistsContract] used cycles: 657243 < 670K
    helper::check_cycles("Deploy CallNonExistsContract", run_result.cycles, 950_000);

    let contract = MockContractInfo::create(&from_eth_address, 0);
    let contract_script_hash = contract.script_hash;
    let contract_account_id = state
        .get_account_id_by_script_hash(&contract_script_hash)
        .unwrap()
        .unwrap();
    let block_info = new_block_info(block_producer_id, block_number, block_number);
    {
        // Call CallNonExistsContract.rawCall(address addr)
        /* abi.encodeWithSignature("rawCall") => 56c94e70
        ethabi_addr: 000000000000000000000000ffffffffffffffffffffffffffffffffffffffff" */
        let input =
            hex::decode("56c94e70000000000000000000000000ffffffffffffffffffffffffffffffffffffffff")
                .unwrap();
        let args = PolyjuiceArgsBuilder::default()
            .gas_limit(73000)
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
        // [contract debug]: [handle_message] Warn: Call non-exists address
        let run_result = generator
            .execute_transaction(
                &ChainView::new(&db, tip_block_hash),
                &mut state,
                &block_info,
                &raw_tx,
                L2TX_MAX_CYCLES,
                None,
            )
            .expect("non_existing_account_address => success with '0x' return_data");
        assert_eq!(
            run_result.return_data,
            vec![
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0
            ]
        );
    }
    {
        // Call CallNonExistsContract.rawCall(address eoa_addr)
        let eoa_addr = [2u8; 20];
        let (_, _script_hash) = create_eth_eoa_account(&mut state, &eoa_addr, 0u64.into());
        let eoa_ethabi_addr = eth_addr_to_ethabi_addr(&eoa_addr);
        let input = hex::decode(format!("56c94e70{}", hex::encode(eoa_ethabi_addr))).unwrap();
        println!("{}", hex::encode(&input));
        let args = PolyjuiceArgsBuilder::default()
            .gas_limit(73000)
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
        // [handle_message] Don't run evm and return empty data
        let run_result = generator
            .execute_transaction(
                &ChainView::new(&db, tip_block_hash),
                &mut state,
                &block_info,
                &raw_tx,
                L2TX_MAX_CYCLES,
                None,
            )
            .expect("empty contract code for account (EoA account)");
        /* The functions call, delegatecall and staticcall all take a single bytes memory parameter
           and return the success condition (as a bool) and the returned data (bytes memory).
        > https://docs.soliditylang.org/en/latest/types.html?highlight=address#members-of-addresses
        */
        assert_eq!(
            run_result.return_data,
            vec![
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0
            ]
        );
        // debug mode: [execute tx] VM machine_run time: 16ms, exit code: 0 used_cycles: 1107696
        // [CallNonExistsContract.rawCall(address eoa_addr)] used cycles: 862060 < 870K
        helper::check_cycles(
            "CallNonExistsContract.rawCall(address eoa_addr)",
            run_result.cycles,
            1_100_000,
        );
    }
}
