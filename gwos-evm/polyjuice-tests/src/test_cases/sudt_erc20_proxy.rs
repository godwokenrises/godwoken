//! Test ERC20 contract
//!   See ./evm-contracts/ERC20.bin

use crate::helper::{
    self, build_eth_l2_script, build_l2_sudt_script, deploy, eth_addr_to_ethabi_addr,
    new_block_info, new_contract_account_script, print_gas_used, setup, PolyjuiceArgsBuilder,
    CKB_SUDT_ACCOUNT_ID, CREATOR_ACCOUNT_ID, FATAL_PRECOMPILED_CONTRACTS, L2TX_MAX_CYCLES,
    SUDT_ERC20_PROXY_USER_DEFINED_DECIMALS_CODE,
};
use crate::DummyState;
use gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID;
use gw_common::registry_address::RegistryAddress;
use gw_common::state::State;
use gw_generator::{error::TransactionError, traits::StateExt, Generator};
use gw_store::state::traits::JournalDB;
use gw_store::traits::chain_store::ChainStore;
use gw_store::{chain_view::ChainView, Store};
use gw_types::{bytes::Bytes, packed::RawL2Transaction, prelude::*, U256};

fn test_sudt_erc20_proxy_inner(
    generator: &Generator,
    store: &Store,
    state: &mut DummyState,
    new_sudt_id: u32,
    decimals: Option<u8>,
) -> anyhow::Result<()> {
    let decimals = decimals.unwrap_or(18);
    let block_producer_id = crate::helper::create_block_producer(state);

    let from_eth_address1 = [1u8; 20];
    let (from_id1, _from_script_hash1) =
        helper::create_eth_eoa_account(state, &from_eth_address1, 2000000u64.into());
    let from_reg_addr1 = RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, from_eth_address1.to_vec());

    let from_eth_address2 = [2u8; 20];
    let (_from_id2, _from_script_hash2) =
        helper::create_eth_eoa_account(state, &from_eth_address2, 2000000u64.into());
    let from_reg_addr2 = RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, from_eth_address2.to_vec());

    let from_eth_address3 = [3u8; 20];
    let (from_id3, _from_script_hash3) =
        helper::create_eth_eoa_account(state, &from_eth_address3, 2000000u64.into());
    let from_reg_addr3 = RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, from_eth_address3.to_vec());

    // Deploy SudtERC20Proxy_UserDefinedDecimals
    // encodeDeploy(["erc20_decimals", "DEC", BigNumber.from(9876543210), 1, 8])
    // => 0x00000000000000000000000000000000000000000000000000000000000000a000000000000000000000000000000000000000000000000000000000000000e0000000000000000000000000000000000000000000000000000000024cb016ea00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000008000000000000000000000000000000000000000000000000000000000000000e65726332305f646563696d616c7300000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000034445430000000000000000000000000000000000000000000000000000000000
    let args = format!("00000000000000000000000000000000000000000000000000000000000000a000000000000000000000000000000000000000000000000000000000000000e0000000000000000000000000000000000000000000000000000000024cb016ea00000000000000000000000000000000000000000000000000000000000000{:02x}00000000000000000000000000000000000000000000000000000000000000{:02x}000000000000000000000000000000000000000000000000000000000000000e65726332305f646563696d616c7300000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000034445430000000000000000000000000000000000000000000000000000000000", new_sudt_id, decimals);
    let init_code = format!("{}{}", SUDT_ERC20_PROXY_USER_DEFINED_DECIMALS_CODE, args);
    let run_result = deploy(
        generator,
        store,
        state,
        CREATOR_ACCOUNT_ID,
        from_id1,
        init_code.as_str(),
        255908,
        0,
        block_producer_id.clone(),
        1,
    );
    // print!("SudtERC20Proxy_UserDefinedDecimals.ContractCode.hex: 0x");
    for byte in run_result.return_data {
        print!("{:02x}", byte);
    }
    println!();
    print_gas_used("Deploy SUDT_ERC20_PROXY contract: ", &run_result.logs);

    let contract_account_script =
        new_contract_account_script(state, from_id1, &from_eth_address1, false);
    let script_hash = contract_account_script.hash().into();
    let new_account_id = state
        .get_account_id_by_script_hash(&script_hash)
        .unwrap()
        .unwrap();
    let eoa1_hex = hex::encode(eth_addr_to_ethabi_addr(&from_eth_address1));
    let eoa2_hex = hex::encode(eth_addr_to_ethabi_addr(&from_eth_address2));
    let eoa3_hex = hex::encode(eth_addr_to_ethabi_addr(&from_eth_address3));
    state
        .mint_sudt(
            new_sudt_id,
            &from_reg_addr1,
            U256::from(160000000000000000000000000000u128),
        )
        .unwrap();

    assert_eq!(
        state
            .get_sudt_balance(new_sudt_id, &from_reg_addr1)
            .unwrap(),
        U256::from(160000000000000000000000000000u128)
    );
    assert_eq!(
        state
            .get_sudt_balance(new_sudt_id, &from_reg_addr2)
            .unwrap(),
        U256::zero()
    );
    assert_eq!(
        state
            .get_sudt_balance(new_sudt_id, &from_reg_addr3)
            .unwrap(),
        U256::zero()
    );

    let total_supply = {
        let mut buf = [0u8; 32];
        let total_supply = state.get_sudt_total_supply(new_sudt_id).unwrap();
        total_supply.to_big_endian(&mut buf);
        hex::encode(&buf)
    };
    for (idx, (action, from_id, args_str, return_data_str)) in [
        // balanceOf(eoa1)
        (
            "balanceOf(eoa1)",
            from_id1,
            format!("70a08231{}", eoa1_hex),
            "000000000000000000000000000000000000000204fce5e3e250261100000000",
        ),
        //
        (
            "balanceOf(eoa2)",
            from_id1,
            format!("70a08231{}", eoa2_hex),
            "0000000000000000000000000000000000000000000000000000000000000000",
        ),
        // transfer("eoa2", 0x22b)
        (
            "transfer(eoa2, 0x22b)",
            from_id1,
            format!(
                "a9059cbb{}000000000000000000000000000000000000000000000000000000000000022b",
                eoa2_hex
            ),
            "0000000000000000000000000000000000000000000000000000000000000001",
        ),
        // balanceOf(eoa2)
        (
            "balanceOf(eoa2)",
            from_id1,
            format!("70a08231{}", eoa2_hex),
            "000000000000000000000000000000000000000000000000000000000000022b",
        ),
        // transfer("eoa2", 0x219)
        (
            "transfer(eoa2, 0x219)",
            from_id1,
            format!(
                "a9059cbb{}0000000000000000000000000000000000000000000000000000000000000219",
                eoa2_hex
            ),
            "0000000000000000000000000000000000000000000000000000000000000001",
        ),
        //// === Transfer to self ====
        // transfer("eoa1", 0x0)
        (
            "transfer(eoa1, 0x0)",
            from_id1,
            format!(
                "a9059cbb{}0000000000000000000000000000000000000000000000000000000000000000",
                eoa1_hex
            ),
            "0000000000000000000000000000000000000000000000000000000000000001",
        ),
        // transfer("eoa1", 0x219)
        (
            "transfer(eoa1, 0x219)",
            from_id1,
            format!(
                "a9059cbb{}0000000000000000000000000000000000000000000000000000000000000219",
                eoa1_hex
            ),
            "0000000000000000000000000000000000000000000000000000000000000001",
        ),
        // balanceOf(eoa2)
        (
            "balanceOf(eoa2)",
            from_id1,
            format!("70a08231{}", eoa2_hex),
            "0000000000000000000000000000000000000000000000000000000000000444",
        ),
        // balanceOf(eoa1)
        (
            "balanceOf(eoa1)",
            from_id1,
            format!("70a08231{}", eoa1_hex),
            "000000000000000000000000000000000000000204fce5e3e2502610fffffbbc",
        ),
        // approve(eoa3, 0x3e8)
        (
            "approve(eoa3, 0x3e8)",
            from_id1,
            format!(
                "095ea7b3{}00000000000000000000000000000000000000000000000000000000000003e8",
                eoa3_hex
            ),
            "0000000000000000000000000000000000000000000000000000000000000001",
        ),
        // transferFrom(eoa1, eoa2, 0x3e8)
        (
            "transferFrom(eoa1, eoa2, 0x3e8)",
            from_id3,
            format!(
                "23b872dd{}{}00000000000000000000000000000000000000000000000000000000000003e8",
                eoa1_hex, eoa2_hex
            ),
            "0000000000000000000000000000000000000000000000000000000000000001",
        ),
        // balanceOf(eoa1)
        (
            "balanceOf(eoa1)",
            from_id1,
            format!("70a08231{}", eoa1_hex),
            "000000000000000000000000000000000000000204fce5e3e2502610fffff7d4",
        ),
        // balanceOf(eoa2)
        (
            "balanceOf(eoa2)",
            from_id1,
            format!("70a08231{}", eoa2_hex),
            "000000000000000000000000000000000000000000000000000000000000082c",
        ),
        // decimals()
        (
            "decimals()",
            from_id1,
            "313ce567".to_string(),
            &format!(
                "00000000000000000000000000000000000000000000000000000000000000{:02x}",
                decimals
            ),
        ),
        // totalSupply()
        (
            "totalSupply()",
            from_id1,
            "18160ddd".to_string(),
            &total_supply,
        ),
        // transfer 0 to an eth_address without Godwoken account
        (
            "transfer(0xffffffffffffffffffffffffffffffffffffffff, 0x0)",
            from_id1,
            "a9059cbb000000000000000000000000ffffffffffffffffffffffffffffffffffffffff0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            "0000000000000000000000000000000000000000000000000000000000000001",
        ),
        // balanceOf(eoa1)
        (
            "balanceOf(eoa1)",
            from_id1,
            format!("70a08231{}", eoa1_hex),
            "000000000000000000000000000000000000000204fce5e3e2502610fffff7d4",
        ),
        // balanceOf(0xffffffffffffffffffffffffffffffffffffffff)
        (
            "balanceOf(0xffffffffffffffffffffffffffffffffffffffff)",
            from_id1,
            "70a08231000000000000000000000000ffffffffffffffffffffffffffffffffffffffff".to_string(),
            "0000000000000000000000000000000000000000000000000000000000000000",
        ),
        // transfer 0xd4 to an eth_address without Godwoken account
        (
            "transfer(0xffffffffffffffffffffffffffffffffffffffff, 0xd4)",
            from_id1,
            "a9059cbb000000000000000000000000ffffffffffffffffffffffffffffffffffffffff00000000000000000000000000000000000000000000000000000000000000d4".to_string(),
            "0000000000000000000000000000000000000000000000000000000000000001",
        ),
        // balanceOf(eoa1)
        (
            "balanceOf(eoa1)",
            from_id1,
            format!("70a08231{}", eoa1_hex),
            "000000000000000000000000000000000000000204fce5e3e2502610fffff700",
        ),
        // balanceOf(0xffffffffffffffffffffffffffffffffffffffff)
        (
            "balanceOf(0xffffffffffffffffffffffffffffffffffffffff)",
            from_id1,
            "70a08231000000000000000000000000ffffffffffffffffffffffffffffffffffffffff".to_string(),
            "00000000000000000000000000000000000000000000000000000000000000d4",
        ),
    ]
    .iter()
    .enumerate()
    {
        let block_number = 2 + idx as u64;
        let block_info = new_block_info(block_producer_id.clone(), block_number, block_number);
        println!(">> [input]: {}", args_str);
        let input = hex::decode(args_str).unwrap();
        let args = PolyjuiceArgsBuilder::default()
            .gas_limit(89915)
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
        let tip_block_hash = store.get_tip_block_hash().unwrap();
        let t = std::time::Instant::now();
        let run_result = generator.execute_transaction(
            &ChainView::new(&db, tip_block_hash),
            state,
            &block_info,
            &raw_tx,
            L2TX_MAX_CYCLES,
            None
        )?;
        if run_result.exit_code != 0 {
            return Err(anyhow::anyhow!(TransactionError::InvalidExitCode(run_result.exit_code)));
        }
        print_gas_used(&format!("SudtERC20Proxy {}: ", action), &run_result.logs);

        println!(
            "[execute_transaction] {} {}ms",
            action,
            t.elapsed().as_millis()
        );
        println!("used_cycles: {}", run_result.cycles.execution);
        println!("write_values.len: {}", run_result.write_data_hashes.len());
        state.finalise().expect("update state");
        assert_eq!(
            run_result.return_data,
            hex::decode(return_data_str).unwrap()
        );
    }

    // from_id1 transfer to from_id2, invalid amount value
    {
        let args_str = format!(
            "a9059cbb{}000000000000000000000000fff00000ffffffffffffffffffffffffffffffff",
            eoa2_hex
        );
        let block_number = 80;
        let block_info = new_block_info(block_producer_id.clone(), block_number, block_number);
        println!(">> [input]: {}", args_str);
        let input = hex::decode(args_str).unwrap();
        let args = PolyjuiceArgsBuilder::default()
            .gas_limit(80000)
            .gas_price(1)
            .value(0)
            .input(&input)
            .build();
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id1.pack())
            .to_id(new_account_id.pack())
            .args(Bytes::from(args).pack())
            .build();
        let db = &store.begin_transaction();
        let tip_block_hash = store.get_tip_block_hash().unwrap();
        let err_run_result = generator
            .execute_transaction(
                &ChainView::new(&db, tip_block_hash),
                state,
                &block_info,
                &raw_tx,
                L2TX_MAX_CYCLES,
                None,
            )
            .unwrap();
        // by: `revert(0, 0)`
        assert_eq!(err_run_result.exit_code, 2);
    }

    // transfer to self insufficient balance
    {
        let args_str = format!(
            "a9059cbb{}00000000000000000000000000000000ffffffffffffffffffffffffffffffff",
            eoa1_hex
        );
        let block_number = 80;
        let block_info = new_block_info(block_producer_id, block_number, block_number);
        println!(">> [input]: {}", args_str);
        let input = hex::decode(args_str).unwrap();
        let args = PolyjuiceArgsBuilder::default()
            .gas_limit(80000)
            .gas_price(1)
            .value(0)
            .input(&input)
            .build();
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id1.pack())
            .to_id(new_account_id.pack())
            .args(Bytes::from(args).pack())
            .build();
        let db = &store.begin_transaction();
        let tip_block_hash = db.get_tip_block_hash().unwrap();
        let err_run_result = generator
            .execute_transaction(
                &ChainView::new(&db, tip_block_hash),
                state,
                &block_info,
                &raw_tx,
                L2TX_MAX_CYCLES,
                None,
            )
            .unwrap();
        // by: `revert(0, 0)`
        assert_eq!(err_run_result.exit_code, 2);
    }
    Ok(())
}

#[test]
fn test_sudt_erc20_proxy_user_defined_decimals() {
    let (store, mut state, generator) = setup();

    let new_sudt_script = build_l2_sudt_script([0xffu8; 32]);
    let new_sudt_id = state.create_account_from_script(new_sudt_script).unwrap();

    assert_eq!(CKB_SUDT_ACCOUNT_ID, 1);
    assert!(
        test_sudt_erc20_proxy_inner(&generator, &store, &mut state, new_sudt_id, Some(8)).is_ok()
    );
}

#[test]
fn test_error_sudt_id_sudt_erc20_proxy() {
    let (store, mut state, generator) = setup();

    let error_new_sudt_script = build_eth_l2_script(&[0xffu8; 20]);
    let error_new_sudt_id = state
        .create_account_from_script(error_new_sudt_script)
        .unwrap();

    assert_eq!(CKB_SUDT_ACCOUNT_ID, 1);
    assert_eq!(
        test_sudt_erc20_proxy_inner(&generator, &store, &mut state, error_new_sudt_id, None)
            .unwrap_err()
            .downcast_ref::<TransactionError>(),
        Some(&TransactionError::InvalidExitCode(
            FATAL_PRECOMPILED_CONTRACTS
        ))
    );
}
