use std::convert::TryInto;

use crate::{
    ctx::MockChain,
    helper::{check_cycles, MockContractInfo},
};

const MULTICALL3_CODE: &str = include_str!("./evm-contracts/Multicall3.bin");
const MULTICALL3_INPUT: &str = include_str!("./evm-contracts/Multicall3.data");
#[test]
fn multicall3_test() -> anyhow::Result<()> {
    let mut chain = MockChain::setup("..")?;
    let from_addr = [1u8; 20];
    let from_id = chain.create_eoa_account(&from_addr, 10000000.into())?;
    let _ = chain.deploy(from_id, &hex::decode(MULTICALL3_CODE)?, 1000000, 1, 0)?;
    let contract_info = MockContractInfo::create(&from_addr, 0);
    let contract_eth_addr = contract_info.eth_addr.try_into().unwrap();
    let contract_account_id = chain
        .get_account_id_by_eth_address(&contract_eth_addr)?
        .expect("contract account id");

    let eth_addr = hex::encode(&contract_eth_addr);
    const OLD_ADDR: &str = "ca11bde05977b3631167028862be2a173976ca11";
    let input = MULTICALL3_INPUT.trim_end_matches('\n');
    let input = input.replace(OLD_ADDR, &eth_addr);
    let input = hex::decode(input)?;

    chain.set_max_cycles(300_000_000);
    // used_cycles: 276137907
    let result = chain.execute(from_id, contract_account_id, &input, 1000000000, 1, 0)?;
    assert_eq!(result.exit_code, 0);
    check_cycles("Multicall3", result.cycles, 280_000_000);
    Ok(())
}
