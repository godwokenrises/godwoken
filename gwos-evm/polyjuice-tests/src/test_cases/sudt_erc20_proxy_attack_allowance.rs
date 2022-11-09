//! Test ERC20 contract
//!   See ./evm-contracts/ERC20.bin

use crate::helper::{
    self, deploy, eth_addr_to_ethabi_addr, new_block_info, new_contract_account_script, setup,
    PolyjuiceArgsBuilder, CKB_SUDT_ACCOUNT_ID, CREATOR_ACCOUNT_ID, L2TX_MAX_CYCLES,
    SUDT_ERC20_PROXY_USER_DEFINED_DECIMALS_CODE,
};
use gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID;
use gw_common::registry_address::RegistryAddress;
use gw_common::state::State;
use gw_store::traits::chain_store::ChainStore;
use gw_store::{chain_view::ChainView, state::traits::JournalDB};
use gw_types::{bytes::Bytes, packed::RawL2Transaction, prelude::*, U256};

const SUDT_ERC20_PROXY_ATTACK_CODE: &str = include_str!("./evm-contracts/AttackSudtERC20Proxy.bin");

#[test]
fn test_attack_allowance() {
    let (store, mut state, generator) = setup();
    let block_producer_id = crate::helper::create_block_producer(&mut state);

    let mint_balance: u128 = 600000;
    let from_eth_addr = [1u8; 20];
    let (from_id, _from_script_hash) =
        helper::create_eth_eoa_account(&mut state, &from_eth_addr, mint_balance.into());
    let from_reg_addr = RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, from_eth_addr.to_vec());

    let target_eth_addr = [2u8; 20];
    let (_target_id, _target_script_hash) =
        helper::create_eth_eoa_account(&mut state, &target_eth_addr, 0u64.into());
    let target_reg_addr = RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, target_eth_addr.to_vec());

    assert_eq!(
        state
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &from_reg_addr)
            .unwrap(),
        U256::from(mint_balance)
    );

    assert_eq!(
        state
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &target_reg_addr)
            .unwrap(),
        U256::zero()
    );

    let mut block_number = 0;

    println!("================");
    // Deploy SudtERC20Proxy
    {
        // ethabi encode params -v string "test" -v string "tt" -v uint256 000000000000000000000000000000000000000204fce5e3e250261100000000 -v uint256 0000000000000000000000000000000000000000000000000000000000000001
        let args = format!("000000000000000000000000000000000000000000000000000000000000008000000000000000000000000000000000000000000000000000000000000000c0000000000000000000000000000000000000000204fce5e3e25026110000000000000000000000000000000000000000000000000000000000000000000000{:02x}0000000000000000000000000000000000000000000000000000000000000004746573740000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000027474000000000000000000000000000000000000000000000000000000000000", CKB_SUDT_ACCOUNT_ID);
        let init_code = format!("{}{}", SUDT_ERC20_PROXY_USER_DEFINED_DECIMALS_CODE, args);
        let _run_result = deploy(
            &generator,
            &store,
            &mut state,
            CREATOR_ACCOUNT_ID,
            from_id,
            init_code.as_str(),
            1253495,
            0,
            block_producer_id.clone(),
            block_number,
        );
    }
    let proxy_eth_addr = {
        let contract_account_script =
            new_contract_account_script(&state, from_id, &from_eth_addr, false);
        let script_hash = contract_account_script.hash().into();
        let reg_addr = state
            .get_registry_address_by_script_hash(ETH_REGISTRY_ACCOUNT_ID, &script_hash)
            .unwrap()
            .unwrap();
        let mut eth_addr = [0u8; 20];
        eth_addr.copy_from_slice(&reg_addr.address);
        eth_addr
    };
    println!("================");

    // Deploy AttackSudtERC20Proxy
    {
        let args = format!(
            "00000000000000000000000000000000000000000000000000000000000000{:02x}",
            CKB_SUDT_ACCOUNT_ID,
        );
        let init_code = format!("{}{}", SUDT_ERC20_PROXY_ATTACK_CODE, args);
        let _run_result = deploy(
            &generator,
            &store,
            &mut state,
            CREATOR_ACCOUNT_ID,
            from_id,
            init_code.as_str(),
            1012616,
            0,
            block_producer_id.clone(),
            block_number,
        );
    }
    block_number += 1;

    let attack_account_id = {
        let contract_account_script =
            new_contract_account_script(&state, from_id, &from_eth_addr, false);
        let script_hash = contract_account_script.hash().into();
        state
            .get_account_id_by_script_hash(&script_hash)
            .unwrap()
            .unwrap()
    };
    println!("================");

    block_number += 1;

    {
        // AttackSudtERC20Proxy.sol => setAllowance(from_id, target_id, 3e8)
        let block_info = new_block_info(block_producer_id.clone(), block_number, block_number);
        let input = hex::decode(format!(
            "da46098c{}{}{}",
            hex::encode(eth_addr_to_ethabi_addr(&from_eth_addr)),
            hex::encode(eth_addr_to_ethabi_addr(&target_eth_addr)),
            "00000000000000000000000000000000000000000000000000000000000003e8",
        ))
        .unwrap();
        let args = PolyjuiceArgsBuilder::default()
            .gas_limit(44037)
            .gas_price(1)
            .value(0)
            .input(&input)
            .build();
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(attack_account_id.pack())
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
            .expect("construct");
        state.finalise().expect("update state");
    }

    assert_eq!(
        state
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &target_reg_addr)
            .unwrap(),
        U256::zero()
    );

    {
        // AttackSudtERC20Proxy.sol => attack1(from_id, target_id, 100000)
        let block_info = new_block_info(block_producer_id, block_number, block_number);
        let input = hex::decode(format!(
            "7483118f{}{}{}{}",
            hex::encode(eth_addr_to_ethabi_addr(&proxy_eth_addr)),
            hex::encode(eth_addr_to_ethabi_addr(&from_eth_addr)),
            hex::encode(eth_addr_to_ethabi_addr(&target_eth_addr)),
            "00000000000000000000000000000000000000000000000000000000000003e8",
        ))
        .unwrap();
        let args = PolyjuiceArgsBuilder::default()
            .gas_limit(40000)
            .gas_price(1)
            .value(0)
            .input(&input)
            .build();
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(attack_account_id.pack())
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
        assert_eq!(
            run_result.return_data,
            hex::decode("0000000000000000000000000000000000000000000000000000000000000000")
                .unwrap()
        );
        state.finalise().expect("update state");
    }

    assert_eq!(
        state
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &target_reg_addr)
            .unwrap(),
        U256::zero()
    );
}
