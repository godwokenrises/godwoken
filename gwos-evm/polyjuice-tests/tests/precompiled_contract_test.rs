use std::{convert::TryInto, u128};

use lib::{ctx::MockChain,helper::MockContractInfo};
const BIG_EXP_MOD_CODE: &str = include_str!("../../polyjuice-tests/src/test_cases/evm-contracts/BigModExp.bin");
#[test]
fn big_exp_mod_test() -> anyhow::Result<()> {
    let mut chain = MockChain::setup("..")?;
    let eth_address = [9u8; 20];
    let mint_ckb = u128::MAX;
    let from_id = chain.create_eoa_account(&eth_address, mint_ckb.into())?;
    let gas_price = 1;
    let value = 0;
    let code = hex::decode(BIG_EXP_MOD_CODE).expect("decode code");
    let run_result = chain.deploy(from_id, &code, 1_000_000, gas_price, value)?;
    // println!("{:#?}",run_result);
    assert_eq!(run_result.exit_code, 0);
    let input = format!(
        "0ade7a0f{}{}{}{}{}{}",
        "0000000000000000000000000000000000000000000000000000000000000001",
        "000000000000000000000000000000000000000000000000000000000000003f",
        "0000000000000000000000000000000000000000000000000000000000000020",
        "0000000000000000000000000000000000000000000000000000000000000457",
        "0000000000000000000000000000000000000000000000000000000000000457",
        "000000000000000000000000000000000000000000000000000000000000006f",
    );
    let code = hex::decode(input).expect("decode code");

    let contract = MockContractInfo::create(&eth_address, 0);
    let eth_addr = contract.eth_addr.try_into().unwrap();
    let to_id = chain
        .get_account_id_by_eth_address(&eth_addr)?
        .expect("to id");

    let run_result = chain.execute(from_id, to_id, &code, 1_000_000, gas_price, value)?;
    assert_eq!(run_result.exit_code, 0);

    let input = format!(
        "0ade7a0f{}{}{}{}{}{}",
        "0000000000000000000000000000000000000000000000000000000000000020",
        "000000000000000000000000000000000000000000000000000000f00000003f",
        "0000000000000000000000000000000000000000000000000000000000000020",
        "0000000000000000000000000000000000000000000000000000000000000457",
        "0000000000000000000000000000000000000000000000000000000000000457",
        "000000000000000000000000000000000000000000000000000000000000006f",
    );
    let code = hex::decode(input).expect("decode code");

    let run_result = chain.execute(from_id, to_id, &code, 1_000_000, gas_price, value)?;
    // see debug log: "[big_mod_exp_required_gas] content_size overflow"
    // exp_size overflow
    assert_eq!(run_result.exit_code, 2);

    let input = format!(
        "0ade7a0f{}{}{}{}{}{}",
        "0000000000000000000000000000000000000000000000000000000000000020",
        "0000000000000000000000000000000000000000000000000000000000021000", // 132kb
        "0000000000000000000000000000000000000000000000000000000000000020",
        "0000000000000000000000000000000000000000000000000000000000000457",
        "0000000000000000000000000000000000000000000000000000000000000457",
        "000000000000000000000000000000000000000000000000000000000000006f",
    );
    let code = hex::decode(input).expect("decode code");

    let run_result = chain.execute(from_id, to_id, &code, 1_000_000, gas_price, value)?;
    // see debug log: "[big_mod_exp_required_gas] content_size overflow"
    // exp_size overflow
    assert_eq!(run_result.exit_code, 2);

    let input = format!(
        "0ade7a0f{}{}{}{}{}{}",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "0000000000000000000000000000000000000000000000000000000000000020",
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    );
    let code = hex::decode(input).expect("decode code");

    let run_result = chain.execute(from_id, to_id, &code, 1_000_000_000, gas_price, value)?;
    assert_eq!(run_result.exit_code, 3); // out of gas

    Ok(())
}
