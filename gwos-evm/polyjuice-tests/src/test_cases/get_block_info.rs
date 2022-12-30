//! Test get block info
//!   See ./evm-contracts/BlockInfo.sol

use std::convert::TryInto;

use crate::helper::{
    eth_addr_to_ethabi_addr, new_block_info, new_contract_account_script, setup,
    PolyjuiceArgsBuilder, CREATOR_ACCOUNT_ID, L2TX_MAX_CYCLES,
};
use gw_common::state::State;
use gw_store::traits::chain_store::ChainStore;
use gw_store::traits::kv_store::KVStoreWrite;
use gw_store::{chain_view::ChainView, schema::*, state::traits::JournalDB};
use gw_types::{
    bytes::Bytes,
    packed::{RawL2Transaction, Uint64},
    prelude::*,
};

const INIT_CODE: &str = include_str!("./evm-contracts/BlockInfo.bin");

#[test]
fn test_get_block_info() {
    let (store, mut state, generator) = setup();
    let block_producer = crate::helper::create_block_producer(&mut state);
    {
        let genesis_number: Uint64 = 0.pack();
        // See: BlockInfo.sol
        let block_hash = [7u8; 32];
        let mut tx = store.begin_transaction();
        tx.insert_raw(COLUMN_INDEX, genesis_number.as_slice(), &block_hash[..])
            .unwrap();
        tx.commit().unwrap();
        println!("block_hash(0): {:?}", tx.get_block_hash_by_number(0));
    }
    let coinbase_hex = hex::encode(eth_addr_to_ethabi_addr(
        &block_producer.address.clone().try_into().unwrap(),
    ));
    println!("coinbase_hex: 0x{}", coinbase_hex);

    let from_eth_address = [1u8; 20];
    let (from_id, _from_script_hash) =
        crate::helper::create_eth_eoa_account(&mut state, &from_eth_address, 400000u64.into());

    // Deploy BlockInfo
    let mut block_number = 0x05;
    let timestamp: u64 = 0xff33 * 1000;
    let block_info = new_block_info(block_producer.clone(), block_number, timestamp);
    let input = hex::decode(INIT_CODE).unwrap();
    let args = PolyjuiceArgsBuilder::default()
        .do_create(true)
        .gas_limit(193537)
        .gas_price(1)
        .value(0)
        .input(&input)
        .build();
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(CREATOR_ACCOUNT_ID.pack())
        .args(Bytes::from(args).pack())
        .build();
    let db = &store.begin_transaction();
    let tip_block_hash = store.get_tip_block_hash().unwrap();
    let _run_result = generator
        .execute_transaction(
            &ChainView::new(&db, tip_block_hash),
            &mut state,
            &block_info,
            &raw_tx,
            L2TX_MAX_CYCLES,
            None,
        )
        .expect("Deploy BlockInfo");
    state.finalise().expect("update state");
    block_number += 1;
    // println!(
    //     "result {}",
    //     serde_json::to_string_pretty(&RunResult::from(run_result)).unwrap()
    // );

    let contract_account_script =
        new_contract_account_script(&state, from_id, &from_eth_address, false);
    let new_account_id = state
        .get_account_id_by_script_hash(&contract_account_script.hash().into())
        .unwrap()
        .unwrap();

    for (operation, fn_sighash, expected_return_data) in [
        (
            "getGenesisHash()",
            "f6c99388",
            "0707070707070707070707070707070707070707070707070707070707070707",
        ),
        (
            "getDifficulty() => 2500000000000000",
            "b6baffe3",
            "0000000000000000000000000000000000000000000000000008e1bc9bf04000",
        ),
        (
            "getGasLimit()",
            "1a93d1c3",
            "0000000000000000000000000000000000000000000000000000000000bebc20",
        ),
        (
            "getNumber()",
            "f2c9ecd8",
            "0000000000000000000000000000000000000000000000000000000000000005",
        ),
        (
            "getTimestamp()",
            "188ec356",
            "000000000000000000000000000000000000000000000000000000000000ff33",
        ),
        // coinbase_hex => eth_addr_to_ethabi_addr(&aggregator_eth_addr)
        ("getCoinbase()", "d1a82a9d", coinbase_hex.as_str()),
    ]
    .iter()
    {
        let block_info = new_block_info(block_producer.clone(), block_number + 1, timestamp + 1);
        let input = hex::decode(fn_sighash).unwrap();
        let args = PolyjuiceArgsBuilder::default()
            .gas_limit(31000)
            .gas_price(1)
            .value(0)
            .input(&input)
            .build();
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(new_account_id.pack())
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
            .expect(operation);
        assert_eq!(
            run_result.return_data,
            hex::decode(expected_return_data).unwrap(),
            "return data of {}",
            operation
        );
    }
}
