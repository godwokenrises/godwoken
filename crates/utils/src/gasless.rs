use std::convert::TryInto;

use anyhow::{Context as _, Result};
use ethabi::decode;
use gw_config::GaslessTxSupportConfig;

use crate::polyjuice_parser::PolyjuiceParser;

#[derive(Debug, Eq, PartialEq)]
pub struct Fee {
    pub gas_limit: u64,
    pub gas_price: u128,
}

pub fn is_gasless_tx(config: Option<&GaslessTxSupportConfig>, tx: &PolyjuiceParser) -> bool {
    tx.gas_price() == 0
        && tx.to_address().is_some()
        && tx.to_address() == config.map(|c| c.entrypoint_address.as_bytes())
}

pub fn gasless_tx_fee(data: &[u8]) -> Result<Fee> {
    use ethabi::ParamType::*;

    // struct UserOperation {
    //     address callContract;
    //     bytes callData;
    //     uint256 callGasLimit;
    //     uint256 verificationGasLimit;
    //     uint256 maxFeePerGas;
    //     uint256 maxPriorityFeePerGas;
    //     bytes paymasterAndData;
    // }
    let mut tokens = decode(
        &[Tuple(vec![
            Address,
            Bytes,
            Uint(256),
            Uint(256),
            Uint(256),
            Uint(256),
            Bytes,
        ])],
        data,
    )
    .context("decode data")?;

    // Why unwrapping: if ethabi successfully decoded the data, we trust it to
    // give us tokens in the right shape.

    let mut tokens = tokens.remove(0).into_tuple().unwrap();
    assert_eq!(tokens.len(), 7);
    let mut tokens = tokens.drain(2..);

    let call_gas_limit = tokens.next().unwrap().into_uint().unwrap();
    let verification_gas_limit = tokens.next().unwrap().into_uint().unwrap();
    let max_fee_per_gas = tokens.next().unwrap().into_uint().unwrap();

    // when using a Paymaster, the verificationGasLimit is used also to as a
    // limit for the postOp call. our security model might call postOp
    // eventually twice so the verificationGasLimit shoud x3 times.
    let gas_limit = (move || {
        verification_gas_limit
            .checked_mul(3.into())?
            .checked_add(call_gas_limit)?
            .try_into()
            .ok()
    })()
    .context("gas limit overflow")?;
    let gas_price = max_fee_per_gas
        .try_into()
        .ok()
        .context("gas price overflow")?;

    Ok(Fee {
        gas_limit,
        gas_price,
    })
}

#[test]
fn test_gasless_tx_fee() {
    // https://web3playground.io/QmVUNCDSFoPQ9d1npLyEP7oJUJr3tymvX9FU9ikjhJeJSo
    //
    // "callGasLimit": 2563223,
    // "verificationGasLimit": 23747,
    // "maxFeePerGas": 25000,
    const HEX_DATA: &str = "00000000000000000000000000000000000000000000000000000000000000200000000000000000000000001df923e4f009663b0fddc1775dac783b85f432fb00000000000000000000000000000000000000000000000000000000000000e00000000000000000000000000000000000000000000000000000000000271c970000000000000000000000000000000000000000000000000000000000005cc300000000000000000000000000000000000000000000000000000000000061a800000000000000000000000000000000000000000000000000000000000061a800000000000000000000000000000000000000000000000000000000000001200000000000000000000000000000000000000000000000000000000000000002ffff00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000141df923e4f009663b0fddc1775dac783b85f432fb000000000000000000000000";
    let mut data = [0u8; HEX_DATA.len() / 2];
    faster_hex::hex_decode(HEX_DATA.as_bytes(), &mut data).unwrap();

    assert_eq!(
        gasless_tx_fee(&data).unwrap(),
        Fee {
            gas_limit: 23747 * 3 + 2563223,
            gas_price: 25000,
        }
    );
}
