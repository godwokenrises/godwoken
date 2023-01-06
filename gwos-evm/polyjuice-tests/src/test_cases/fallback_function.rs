//! Test FallbackFunction
//!   See ./evm-contracts/FallbackFunction.sol

use crate::helper::{
    self, create_block_producer, new_block_info, setup, simple_storage_get, MockContractInfo,
    PolyjuiceArgsBuilder, CREATOR_ACCOUNT_ID, L2TX_MAX_CYCLES,
};
use gw_common::state::State;
use gw_store::traits::chain_store::ChainStore;
use gw_store::{chain_view::ChainView, state::traits::JournalDB};
use gw_types::{bytes::Bytes, packed::RawL2Transaction, prelude::*};

const INIT_CODE: &str = include_str!("./evm-contracts/FallbackFunction.bin");

#[test]
fn test_fallback_function() {
    let (store, mut state, generator) = setup();
    let block_producer = create_block_producer(&mut state);

    let from_eth_address = [1u8; 20];
    let (from_id, _from_script_hash) =
        crate::helper::create_eth_eoa_account(&mut state, &from_eth_address, 200000u64.into());

    {
        // Deploy FallbackFunction Contract
        let block_info = new_block_info(block_producer.clone(), 1, 0);
        let input = hex::decode(INIT_CODE).unwrap();
        let args = PolyjuiceArgsBuilder::default()
            .do_create(true)
            .gas_limit(79996)
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
        let run_result = generator
            .execute_transaction(
                &ChainView::new(&db, tip_block_hash),
                &mut state,
                &block_info,
                &raw_tx,
                L2TX_MAX_CYCLES,
                None,
            )
            .expect("construct");
        // [Deploy FallbackFunction] used cycles: 587271 < 590K
        helper::check_cycles("Deploy FallbackFunction", run_result.cycles, 920_000);
        state.finalise().expect("update state");
    }

    let contract_account = MockContractInfo::create(&from_eth_address, 0);
    let new_account_id = state
        .get_account_id_by_script_hash(&contract_account.script_hash)
        .unwrap()
        .unwrap();
    let run_result = simple_storage_get(&store, &mut state, &generator, 0, from_id, new_account_id);
    assert_eq!(
        run_result.return_data,
        hex::decode("000000000000000000000000000000000000000000000000000000000000007b").unwrap()
    );

    {
        // Call fallback()
        let block_info = new_block_info(block_producer, 2, 0);
        let input = hex::decode("3333").unwrap();
        let args = PolyjuiceArgsBuilder::default()
            .gas_limit(51144)
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
            .expect("Call fallback()");
        // [Call fallback()] used cycles: 514059 < 520K
        helper::check_cycles("Call fallback()", run_result.cycles, 625_000);
        assert!(run_result.return_data.is_empty());
        state.finalise().expect("update state");
    }

    let run_result = simple_storage_get(&store, &mut state, &generator, 0, from_id, new_account_id);
    assert_eq!(
        run_result.return_data,
        hex::decode("00000000000000000000000000000000000000000000000000000000000003e7").unwrap()
    );
}
