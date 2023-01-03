use crate::helper::{
    self, deploy, new_block_info, MockContractInfo, PolyjuiceArgsBuilder, CREATOR_ACCOUNT_ID,
    L2TX_MAX_CYCLES,
};
use gw_common::state::State;
use gw_store::traits::chain_store::ChainStore;
use gw_types::{
    packed::RawL2Transaction,
    prelude::{Builder, Entity, Pack},
};
use std::convert::TryInto;

const CONTRACT_CODE: &str = include_str!("./evm-contracts/AddressType.bin");

#[test]
fn test_get_contract_code() {
    let (store, mut state, generator) = helper::setup();
    let block_producer_id = helper::create_block_producer(&mut state);

    let from_eth_address = [1u8; 20];
    let (from_id, _from_script_hash) =
        helper::create_eth_eoa_account(&mut state, &from_eth_address, 400000u64.into());

    // Deploy contract
    let mut block_number = 1;
    let _run_result = deploy(
        &generator,
        &store,
        &mut state,
        CREATOR_ACCOUNT_ID,
        from_id,
        CONTRACT_CODE,
        122000,
        0,
        block_producer_id.clone(),
        block_number,
    );
    let contract = MockContractInfo::create(&from_eth_address, 0);
    let contract_id = state
        .get_account_id_by_script_hash(&contract.script_hash)
        .unwrap()
        .expect("get contract account ID by account_script");
    assert!(contract_id >= 6);

    // test createMemoryArray function
    block_number += 1;
    let block_info = new_block_info(block_producer_id.clone(), block_number, block_number);
    let input = hex::decode("c59083f5").expect("createMemoryArray function");
    let args = PolyjuiceArgsBuilder::default()
        .gas_limit(30297)
        .gas_price(1)
        .value(0)
        .input(&input)
        .build();
    let raw_l2tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(contract_id.pack())
        .args(gw_types::bytes::Bytes::from(args).pack())
        .build();
    let db = &store.begin_transaction();
    let tip_block_hash = db.get_tip_block_hash().unwrap();
    let run_result = generator
        .execute_transaction(
            &gw_store::chain_view::ChainView::new(&db, tip_block_hash),
            &mut state,
            &block_info,
            &raw_l2tx,
            L2TX_MAX_CYCLES,
            None,
        )
        .expect("call createMemoryArray function");
    let mut expect_result = [0u8; 32];
    #[allow(clippy::needless_range_loop)]
    for i in 0..32 {
        expect_result[i] = i as u8;
    }
    println!("MemoryArray: {:?}", run_result.return_data);
    assert_eq!(run_result.return_data[64..], expect_result);

    // Try to get the contract code
    block_number += 1;
    let block_info = new_block_info(block_producer_id, block_number, block_number);
    let input = hex::decode("ea879634").expect("getCode function");
    let args = PolyjuiceArgsBuilder::default()
        .gas_limit(25439)
        .gas_price(1)
        .value(0)
        .input(&input)
        .build();
    let raw_l2tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(contract_id.pack())
        .args(gw_types::bytes::Bytes::from(args).pack())
        .build();
    let db = &store.begin_transaction();
    let tip_block_hash = db.get_tip_block_hash().unwrap();
    let run_result = generator
        .execute_transaction(
            &gw_store::chain_view::ChainView::new(&db, tip_block_hash),
            &mut state,
            &block_info,
            &raw_l2tx,
            L2TX_MAX_CYCLES,
            None,
        )
        .expect("call getCode function");
    let expected_code = hex::decode(CONTRACT_CODE).expect("code hex to Vec<u8>");
    let code_len = usize::from_be_bytes(run_result.return_data[56..64].try_into().unwrap());
    assert_eq!(expected_code.len() - 32, code_len);
    assert_eq!(
        run_result.return_data[64..64 + code_len],
        expected_code[32..]
    );
}
