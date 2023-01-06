//! Test SELFDESTRUCT op code
//!   See ./evm-contracts/SelfDestruct.sol

use crate::helper::{
    self, create_block_producer, create_eth_eoa_account, eth_addr_to_ethabi_addr, new_block_info,
    new_contract_account_script, setup, PolyjuiceArgsBuilder, CKB_SUDT_ACCOUNT_ID,
    CREATOR_ACCOUNT_ID, L2TX_MAX_CYCLES,
};
use gw_common::{
    builtins::ETH_REGISTRY_ACCOUNT_ID, registry_address::RegistryAddress, state::State,
};
use gw_store::traits::chain_store::ChainStore;
use gw_store::{chain_view::ChainView, state::traits::JournalDB};
use gw_types::{bytes::Bytes, packed::RawL2Transaction, prelude::*, U256};

const INIT_CODE: &str = include_str!("./evm-contracts/SelfDestruct.bin");

#[test]
fn test_selfdestruct() {
    let (store, mut state, generator) = setup();
    let block_producer = create_block_producer(&mut state);

    let from_eth_address = [1u8; 20];
    let (from_id, _from_script_hash) =
        create_eth_eoa_account(&mut state, &from_eth_address, 200000u64.into());

    let beneficiary_eth_addr = [2u8; 20];
    let beneficiary_ethabi_addr = eth_addr_to_ethabi_addr(&beneficiary_eth_addr);
    let (_beneficiary_id, _beneficiary_script_hash) =
        create_eth_eoa_account(&mut state, &beneficiary_eth_addr, 0u64.into());
    let beneficiary_reg_addr =
        RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, beneficiary_eth_addr.to_vec());
    assert_eq!(
        state
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &beneficiary_reg_addr)
            .unwrap(),
        U256::zero()
    );

    {
        // Deploy SelfDestruct
        let block_info = new_block_info(block_producer.clone(), 1, 0);
        let mut input = hex::decode(INIT_CODE).unwrap();
        input.extend(beneficiary_ethabi_addr);
        let args = PolyjuiceArgsBuilder::default()
            .do_create(true)
            .gas_limit(79933)
            .gas_price(1)
            .value(200)
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
        // [Deploy SelfDestruct] used cycles: 570570 < 580K
        helper::check_cycles("Deploy SelfDestruct", run_result.cycles, 900_000);
        state.finalise().expect("update state");
    }

    let contract_account_script =
        new_contract_account_script(&state, from_id, &from_eth_address, false);
    let new_script_hash = contract_account_script.hash();
    let contract_reg_addr = state
        .get_registry_address_by_script_hash(ETH_REGISTRY_ACCOUNT_ID, &new_script_hash.into())
        .unwrap()
        .unwrap();
    let new_account_id = state
        .get_account_id_by_script_hash(&contract_account_script.hash().into())
        .unwrap()
        .unwrap();
    assert_eq!(
        state.get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &contract_reg_addr),
        Ok(U256::from(200u64))
    );
    assert_eq!(
        state
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &beneficiary_reg_addr)
            .unwrap(),
        U256::zero()
    );
    {
        // call SelfDestruct.done();
        let block_info = new_block_info(block_producer.clone(), 2, 0);
        let input = hex::decode("ae8421e1").unwrap();
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
            .expect("construct");
        // [call SelfDestruct.done()] used cycles: 589657 < 600K
        helper::check_cycles("call SelfDestruct.done()", run_result.cycles, 740_000);
        state.finalise().expect("update state");
    }
    assert_eq!(
        state
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &contract_reg_addr)
            .unwrap(),
        U256::zero()
    );
    assert_eq!(
        state
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &beneficiary_reg_addr)
            .unwrap(),
        U256::from(200u64)
    );

    {
        // call SelfDestruct.done();
        let block_info = new_block_info(block_producer, 2, 0);
        let input = hex::decode("ae8421e1").unwrap();
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
        let result = generator
            .execute_transaction(
                &ChainView::new(&db, tip_block_hash),
                &mut state,
                &block_info,
                &raw_tx,
                L2TX_MAX_CYCLES,
                None,
            )
            .unwrap();
        assert_eq!(result.exit_code, -50);
    }
}
