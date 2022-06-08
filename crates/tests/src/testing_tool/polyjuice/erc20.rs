use gw_common::registry_address::RegistryAddress;
use gw_types::U256;

use super::{abi_encode_eth_address, PolyjuiceArgsBuilder};

const SUDT_ERC20_BIN: &str = include_str!("./SudtERC20Proxy_UserDefinedDecimals.bin");

pub struct SudtErc20ArgsBuilder;

impl SudtErc20ArgsBuilder {
    pub fn deploy(sudt_id: u32, decimals: u32) -> PolyjuiceArgsBuilder {
        let constructor = format!("00000000000000000000000000000000000000000000000000000000000000a000000000000000000000000000000000000000000000000000000000000000e0000000000000000000000000000000000000000000000000000000024cb016ea00000000000000000000000000000000000000000000000000000000000000{:02x}00000000000000000000000000000000000000000000000000000000000000{:02x}000000000000000000000000000000000000000000000000000000000000000e65726332305f646563696d616c7300000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000034445430000000000000000000000000000000000000000000000000000000000", sudt_id, decimals);

        let data = {
            let bin = format!("{}{}", SUDT_ERC20_BIN, constructor);
            hex::decode(bin).unwrap()
        };

        PolyjuiceArgsBuilder::default()
            .create(true)
            .gas_limit(270000)
            .gas_price(1)
            .value(0)
            .data(data)
    }

    pub fn transfer(to: &RegistryAddress, amount: U256) -> PolyjuiceArgsBuilder {
        let address = hex::encode(&abi_encode_eth_address(to));
        let amount = {
            // U256 doesn't implement pad
            let hex_amount = format!("{:x}", amount);
            format!("{:0>64}", hex_amount)
        };
        let data = {
            let call = format!("a9059cbb{}{}", address, amount);
            hex::decode(call).unwrap()
        };

        PolyjuiceArgsBuilder::default()
            .gas_limit(40000)
            .gas_price(1)
            .value(0)
            .data(data)
    }

    pub fn balance_of(registry_address: &RegistryAddress) -> PolyjuiceArgsBuilder {
        let address = hex::encode(&abi_encode_eth_address(registry_address));
        let data = {
            let sig = format!("70a08231{}", address);
            hex::decode(sig).unwrap()
        };

        PolyjuiceArgsBuilder::default()
            .gas_limit(40000)
            .gas_price(1)
            .value(0)
            .data(data)
    }
}
