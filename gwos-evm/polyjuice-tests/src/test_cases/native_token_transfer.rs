use std::convert::TryInto;

use gw_types::{
    packed::RawL2Transaction,
    prelude::{Builder, Entity, Pack},
};

use crate::{
    ctx::MockChain,
    helper::{parse_log, Log, MockContractInfo, PolyjuiceArgsBuilder, CREATOR_ACCOUNT_ID},
};

#[test]
fn native_token_transfer_test() -> anyhow::Result<()> {
    let mut chain = MockChain::setup("..")?;
    let mint = 100000.into();
    let from_addr = [1u8; 20];
    let from_id = chain.create_eoa_account(&from_addr, mint)?;
    let to_addr = [2u8; 20];
    let _to_id = chain.create_eoa_account(&to_addr, mint)?;

    let value = 400;
    let args = PolyjuiceArgsBuilder::default()
        .gas_price(1)
        .gas_limit(100000)
        .value(value)
        .to_address(to_addr)
        .build();
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(CREATOR_ACCOUNT_ID.pack())
        .args(ckb_vm::Bytes::from(args).pack())
        .build();
    let run_result = chain.execute_raw(raw_tx)?;
    assert_eq!(run_result.exit_code, crate::constant::EVMC_SUCCESS);

    let from_balance = chain.get_balance(&from_addr)?;
    let to_balance = chain.get_balance(&to_addr)?;
    println!("from balance: {}, to balance: {}", from_balance, to_balance);
    assert_eq!(mint - 21000 - value, from_balance);
    assert_eq!(mint + value, to_balance);

    Ok(())
}

#[test]
fn native_token_transfer_unregistered_address_test() -> anyhow::Result<()> {
    let mut chain = MockChain::setup("..")?;
    let mint = 100000.into();
    let from_addr = [1u8; 20];
    let from_id = chain.create_eoa_account(&from_addr, mint)?;
    let to_addr = [2u8; 20];

    let value = 400;
    let args = PolyjuiceArgsBuilder::default()
        .gas_price(1)
        .gas_limit(100000)
        .value(value)
        .to_address(to_addr)
        .build();
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(CREATOR_ACCOUNT_ID.pack())
        .args(ckb_vm::Bytes::from(args).pack())
        .build();
    let run_result = chain.execute_raw(raw_tx)?;
    assert_eq!(run_result.exit_code, crate::constant::EVMC_SUCCESS);

    let system_log = run_result.logs.last().map(parse_log);
    if let Some(Log::PolyjuiceSystem { gas_used, .. }) = system_log {
        assert_eq!(gas_used, 21000 + 25000);
    }

    let account_id = chain.get_account_id_by_eth_address(&to_addr)?;
    assert_eq!(Some(6), account_id);
    let from_balance = chain.get_balance(&from_addr)?;
    let to_balance = chain.get_balance(&to_addr)?;
    println!("from balance: {}, to balance: {}", from_balance, to_balance);
    assert_eq!(mint - 21000 - 25000 - value, from_balance);
    assert_eq!(value, to_balance.as_u128());

    Ok(())
}

#[test]
fn native_token_transfer_contract_address_test() -> anyhow::Result<()> {
    let mut chain = MockChain::setup("..")?;
    let mint = 100000.into();
    let from_addr = [1u8; 20];
    let from_id = chain.create_eoa_account(&from_addr, mint)?;

    let code = include_str!("./evm-contracts/SimpleStorage.bin");
    let code = hex::decode(code)?;
    chain.deploy(from_id, &code, 100000, 1, 0)?;
    let from_balance = chain.get_balance(&from_addr)?;
    let contract_info = MockContractInfo::create(&from_addr, 0);
    let contract_eth_addr = contract_info.eth_addr.try_into().unwrap();
    let value = 400;
    let args = PolyjuiceArgsBuilder::default()
        .gas_price(1)
        .gas_limit(100000)
        .value(value)
        .to_address(contract_eth_addr)
        .build();
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(CREATOR_ACCOUNT_ID.pack())
        .args(ckb_vm::Bytes::from(args).pack())
        .build();
    let run_result = chain.execute_raw(raw_tx)?;
    assert_eq!(run_result.exit_code, -94); // ERROR_NATIVE_TOKEN_TRANSFER = -94

    let from_balance_after = chain.get_balance(&from_addr)?;
    let to_balance = chain.get_balance(&contract_eth_addr)?;
    println!("from balance: {}, to balance: {}", from_balance, to_balance);
    assert_eq!(from_balance - 21000, from_balance_after);

    Ok(())
}

#[test]
fn native_token_transfer_invalid_to_id_test() -> anyhow::Result<()> {
    let mut chain = MockChain::setup("..")?;
    let mint = 100000.into();
    let from_addr = [1u8; 20];
    let from_id = chain.create_eoa_account(&from_addr, mint)?;
    let to_addr = [2u8; 20];
    let _to_id = chain.create_eoa_account(&to_addr, mint)?;

    let code = include_str!("./evm-contracts/SimpleStorage.bin");
    let code = hex::decode(code)?;
    chain.deploy(from_id, &code, 100000, 1, 0)?;
    let from_balance = chain.get_balance(&from_addr)?;
    let contract_info = MockContractInfo::create(&from_addr, 0);
    let contract_eth_addr = contract_info.eth_addr.try_into().unwrap();
    let contract_account_id = chain
        .get_account_id_by_eth_address(&contract_eth_addr)?
        .expect("get account id");
    println!("contract account id: {}", contract_account_id);
    let value = 400;
    let args = PolyjuiceArgsBuilder::default()
        .gas_price(1)
        .gas_limit(100000)
        .value(value)
        .to_address(to_addr)
        .build();
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(contract_account_id.pack())
        .args(ckb_vm::Bytes::from(args).pack())
        .build();

    let run_result = chain.execute_raw(raw_tx)?;
    assert_eq!(run_result.exit_code, crate::constant::EVMC_SUCCESS);

    let to_balance = chain.get_balance(&to_addr)?;
    println!("from balance: {}, to balance: {}", from_balance, to_balance);
    assert_eq!(mint, to_balance);
    assert_eq!(value, chain.get_balance(&contract_eth_addr)?.as_u128());

    Ok(())
}

#[test]
fn native_token_transfer_invalid_to_id_and_unregistered_address_test() -> anyhow::Result<()> {
    let mut chain = MockChain::setup("..")?;
    let mint = 100000.into();
    let from_addr = [1u8; 20];
    let from_id = chain.create_eoa_account(&from_addr, mint)?;
    let to_addr = [2u8; 20];

    let code = include_str!("./evm-contracts/SimpleStorage.bin");
    let code = hex::decode(code)?;
    chain.deploy(from_id, &code, 100000, 1, 0)?;
    let contract_info = MockContractInfo::create(&from_addr, 0);
    let contract_eth_addr = contract_info.eth_addr.try_into().unwrap();
    let contract_account_id = chain
        .get_account_id_by_eth_address(&contract_eth_addr)?
        .expect("get account id");
    println!("contract account id: {}", contract_account_id);
    let value = 400;
    let args = PolyjuiceArgsBuilder::default()
        .gas_price(1)
        .gas_limit(100000)
        .value(value)
        .to_address(to_addr)
        .build();
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(contract_account_id.pack())
        .args(ckb_vm::Bytes::from(args).pack())
        .build();
    let run_result = chain.execute_raw(raw_tx)?;
    assert_eq!(run_result.exit_code, crate::constant::EVMC_SUCCESS);

    let to_id = chain.get_account_id_by_eth_address(&to_addr)?;
    assert_eq!(None, to_id);
    assert_eq!(value, chain.get_balance(&contract_eth_addr)?.as_u128());

    Ok(())
}

#[test]
fn native_token_transfer_unregistered_zero_address_test() -> anyhow::Result<()> {
    let mut chain = MockChain::setup("..")?;
    let mint = 100000.into();
    let from_addr = [1u8; 20];
    let from_id = chain.create_eoa_account(&from_addr, mint)?;
    let to_addr = [0u8; 20];

    let value = 400;
    let args = PolyjuiceArgsBuilder::default()
        .gas_price(1)
        .gas_limit(100000)
        .value(value)
        .to_address(to_addr)
        .build();
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(CREATOR_ACCOUNT_ID.pack())
        .args(ckb_vm::Bytes::from(args).pack())
        .build();
    let run_result = chain.execute_raw(raw_tx)?;
    assert_eq!(run_result.exit_code, crate::constant::EVMC_SUCCESS);

    let from_balance = chain.get_balance(&from_addr)?;
    let to_balance = chain.get_balance(&to_addr)?;
    println!("from balance: {}, to balance: {}", from_balance, to_balance);
    assert_eq!(mint - 21000 - 25000 - value, from_balance);
    assert_eq!(value, to_balance.as_u128());

    Ok(())
}
