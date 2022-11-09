use std::convert::TryInto;

use anyhow::Ok;

use crate::{ctx::MockChain, helper::MockContractInfo};
const REVERT_CODE: &str = include_str!("./evm-contracts/Revert.bin");
const CALL_REVERT_CODE: &str = include_str!("./evm-contracts/CallRevertWithTryCatch.bin");
const CALL_DEEP_REVERT_CODE: &str =
    include_str!("./evm-contracts/CallRevertWithTryCatchInDepth.bin");
const CONSTRUCTOR_REVERT_CODE: &str =
    include_str!("./evm-contracts/CallRevertWithTryCatchInConstructor.bin");
const CALL_REVERT_WO_TRY: &str = include_str!("./evm-contracts/CallRevertWithoutTryCatch.bin");
#[test]
fn revert_in_try_test() -> anyhow::Result<()> {
    let mut chain = MockChain::setup("..")?;
    let eth_address = [9u8; 20];
    let mint_ckb = 1_000_000;
    let from_id = chain.create_eoa_account(&eth_address, mint_ckb.into())?;
    //deploy contracts
    let gas_limit = 100000;
    let gas_price = 1;
    let value = 0;
    let code = hex::decode(REVERT_CODE).expect("decode code");
    let run_result = chain.deploy(from_id, &code, gas_limit, gas_price, value)?;
    assert_eq!(run_result.exit_code, 0);
    let code = hex::decode(CALL_REVERT_CODE).expect("decode code");
    let run_result = chain.deploy(from_id, &code, gas_limit, gas_price, value)?;
    assert_eq!(run_result.exit_code, 0);

    let revert_contract = MockContractInfo::create(&eth_address, 0);
    let revert_eth_addr = revert_contract.eth_addr.try_into().unwrap();
    let revert_id = chain
        .get_account_id_by_eth_address(&revert_eth_addr)?
        .expect("to id");
    let call_revert_contract = MockContractInfo::create(&eth_address, 1);
    let call_revert_eth_addr = call_revert_contract.eth_addr.try_into().unwrap();
    let call_revert_id = chain
        .get_account_id_by_eth_address(&call_revert_eth_addr)?
        .expect("to id");

    //call CallRevertWithTryCatch.test(Revert)
    let args_str = format!(
        "bb29998e000000000000000000000000{}",
        hex::encode(&revert_eth_addr)
    );
    let code = hex::decode(args_str)?;
    let run_result = chain.execute(from_id, call_revert_id, &code, gas_limit, gas_price, value)?;
    assert_eq!(run_result.exit_code, 0);

    //check state
    let args_str = "c19d93fb"; //state()
    let code = hex::decode(args_str)?;
    let run_result = chain.execute(from_id, revert_id, &code, gas_limit, gas_price, value)?;
    assert_eq!(run_result.exit_code, 0);
    let state = hex::encode(run_result.return_data);
    let state = state.parse::<u32>().unwrap();
    assert_eq!(state, 1);

    let run_result = chain.execute(from_id, call_revert_id, &code, gas_limit, gas_price, value)?;
    assert_eq!(run_result.exit_code, 0);
    let state = hex::encode(run_result.return_data);
    let state = state.parse::<u32>().unwrap();
    assert_eq!(state, 4);
    Ok(())
}
#[test]
fn revert_in_deep_try_test() -> anyhow::Result<()> {
    let mut chain = MockChain::setup("..")?;
    let eth_address = [9u8; 20];
    let mint_ckb = 1_000_000_000;
    let from_id = chain.create_eoa_account(&eth_address, mint_ckb.into())?;
    //deploy contracts
    let gas_limit = 1000000;
    let gas_price = 1;
    let value = 0;
    let code = hex::decode(REVERT_CODE).expect("decode code");
    let run_result = chain.deploy(from_id, &code, gas_limit, gas_price, value)?;
    assert_eq!(run_result.exit_code, 0);
    let code = hex::decode(CALL_REVERT_CODE).expect("decode code");
    let run_result = chain.deploy(from_id, &code, gas_limit, gas_price, value)?;
    assert_eq!(run_result.exit_code, 0);
    let code = hex::decode(CALL_DEEP_REVERT_CODE).expect("decode code");
    let run_result = chain.deploy(from_id, &code, gas_limit, gas_price, value)?;
    assert_eq!(run_result.exit_code, 0);

    let revert_contract = MockContractInfo::create(&eth_address, 0);
    let revert_eth_addr = revert_contract.eth_addr.try_into().unwrap();
    let revert_id = chain
        .get_account_id_by_eth_address(&revert_eth_addr)?
        .expect("to id");
    let call_revert_contract = MockContractInfo::create(&eth_address, 1);
    let call_revert_eth_addr = call_revert_contract.eth_addr.try_into().unwrap();
    let call_revert_id = chain
        .get_account_id_by_eth_address(&call_revert_eth_addr)?
        .expect("to id");
    let call_depth_revert_contract = MockContractInfo::create(&eth_address, 2);
    let call_depth_revert_eth_addr = call_depth_revert_contract.eth_addr.try_into().unwrap();
    let call_depth_revert_id = chain
        .get_account_id_by_eth_address(&call_depth_revert_eth_addr)?
        .expect("to id");

    //CallRevertWithTryCatchInDepth.test(CallRevertWithTryCatch, Revert)
    let args_str = format!(
        "2b6d0ceb000000000000000000000000{}000000000000000000000000{}",
        hex::encode(&call_revert_eth_addr),
        hex::encode(&revert_eth_addr)
    );
    let code = hex::decode(args_str)?;
    let run_result = chain.execute(
        from_id,
        call_depth_revert_id,
        &code,
        gas_limit,
        gas_price,
        value,
    )?;
    assert_eq!(run_result.exit_code, 0);

    //check state
    let args_str = "c19d93fb"; //state()
    let code = hex::decode(args_str)?;
    let run_result = chain.execute(from_id, revert_id, &code, gas_limit, gas_price, value)?;
    assert_eq!(run_result.exit_code, 0);
    let state = hex::encode(run_result.return_data);
    let state = state.parse::<u32>().unwrap();
    assert_eq!(state, 1);

    let run_result = chain.execute(from_id, call_revert_id, &code, gas_limit, gas_price, value)?;
    assert_eq!(run_result.exit_code, 0);
    let state = hex::encode(run_result.return_data);
    let state = state.parse::<u32>().unwrap();
    assert_eq!(state, 4);

    let run_result = chain.execute(
        from_id,
        call_depth_revert_id,
        &code,
        gas_limit,
        gas_price,
        value,
    )?;
    assert_eq!(run_result.exit_code, 0);
    let state = hex::encode(run_result.return_data);
    let state = state.parse::<u32>().unwrap();
    assert_eq!(state, 3);
    Ok(())
}

#[test]
fn revert_contructor_try_test() -> anyhow::Result<()> {
    let mut chain = MockChain::setup("..")?;
    let eth_address = [9u8; 20];
    let mint_ckb = 1_000_000;
    let from_id = chain.create_eoa_account(&eth_address, mint_ckb.into())?;
    //deploy contracts
    let gas_limit = 1000000;
    let gas_price = 1;
    let value = 0;
    let code = hex::decode(REVERT_CODE).expect("decode code");
    let run_result = chain.deploy(from_id, &code, gas_limit, gas_price, value)?;
    let revert_contract = MockContractInfo::create(&eth_address, 0);
    let revert_eth_addr = revert_contract.eth_addr.try_into().unwrap();
    let revert_id = chain
        .get_account_id_by_eth_address(&revert_eth_addr)?
        .expect("to id");
    assert_eq!(run_result.exit_code, 0);

    let deploy_args = format!("000000000000000000000000{}", hex::encode(&revert_eth_addr));
    let code = format!("{}{}", CONSTRUCTOR_REVERT_CODE, deploy_args);
    let code = hex::decode(code).expect("decode code");
    let run_result = chain.deploy(from_id, &code, gas_limit, gas_price, value)?;
    assert_eq!(run_result.exit_code, 0);

    let constructor_revert_contract = MockContractInfo::create(&eth_address, 1);
    let constructor_revert_eth_addr = constructor_revert_contract.eth_addr.try_into().unwrap();
    let constructor_revert_id = chain
        .get_account_id_by_eth_address(&constructor_revert_eth_addr)?
        .expect("to id");

    // check if failed try state(Revert.state) is reverted
    let args_str = "c19d93fb";
    let code = hex::decode(args_str)?;
    let run_result = chain.execute(from_id, revert_id, &code, gas_limit, gas_price, value)?;
    assert_eq!(run_result.exit_code, 0);
    let state = hex::encode(run_result.return_data);
    let state = state.parse::<u32>().unwrap();
    assert_eq!(state, 1);

    // check if failed try catch state (CallRevertWithTryCatchInConstructor.state) is updated
    let run_result = chain.execute(
        from_id,
        constructor_revert_id,
        &code,
        gas_limit,
        gas_price,
        value,
    )?;
    assert_eq!(run_result.exit_code, 0);
    let state = hex::encode(run_result.return_data);
    let state = state.parse::<u32>().unwrap();
    assert_eq!(state, 4);
    Ok(())
}

#[test]
fn revert_test() -> anyhow::Result<()> {
    let mut chain = MockChain::setup("..")?;
    let eth_address = [9u8; 20];
    let mint_ckb = 1_000_000;
    let from_id = chain.create_eoa_account(&eth_address, mint_ckb.into())?;
    //deploy contracts
    let gas_limit = 100000;
    let gas_price = 1;
    let value = 0;
    let code = hex::decode(REVERT_CODE).expect("decode code");
    let run_result = chain.deploy(from_id, &code, gas_limit, gas_price, value)?;
    assert_eq!(run_result.exit_code, 0);
    let code = hex::decode(CALL_REVERT_WO_TRY).expect("decode code");
    let run_result = chain.deploy(from_id, &code, gas_limit, gas_price, value)?;
    assert_eq!(run_result.exit_code, 0);

    let revert_contract = MockContractInfo::create(&eth_address, 0);
    let revert_eth_addr = revert_contract.eth_addr.try_into().unwrap();
    let revert_id = chain
        .get_account_id_by_eth_address(&revert_eth_addr)?
        .expect("to id");
    let call_revert_contract = MockContractInfo::create(&eth_address, 1);
    let call_revert_eth_addr = call_revert_contract.eth_addr.try_into().unwrap();
    let call_revert_id = chain
        .get_account_id_by_eth_address(&call_revert_eth_addr)?
        .expect("to id");

    //call CallRevertWithoutTryCatch.test(Revert)
    let args_str = format!(
        "bb29998e000000000000000000000000{}",
        hex::encode(&revert_eth_addr)
    );
    let code = hex::decode(args_str)?;
    let run_result = chain.execute(from_id, call_revert_id, &code, gas_limit, gas_price, value)?;
    assert_eq!(run_result.exit_code, 2);

    //check state
    let args_str = "c19d93fb"; //state()
    let code = hex::decode(args_str)?;
    let run_result = chain.execute(from_id, revert_id, &code, gas_limit, gas_price, value)?;
    assert_eq!(run_result.exit_code, 0);
    let state = hex::encode(run_result.return_data);
    let state = state.parse::<u32>().unwrap();
    assert_eq!(state, 1);

    let run_result = chain.execute(from_id, call_revert_id, &code, gas_limit, gas_price, value)?;
    assert_eq!(run_result.exit_code, 0);
    let state = hex::encode(run_result.return_data);
    let state = state.parse::<u32>().unwrap();
    assert_eq!(state, 1);
    Ok(())
}
