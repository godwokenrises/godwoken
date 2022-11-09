## sUDT-ERC20 Proxy Contract

The [sUDT-ERC20 Proxy Contract](./SudtERC20Proxy_UserDefinedDecimals.sol) is a special smart contract written in Solidity, which is designed to utilize the Godwoken and Polyjuice frameworks. This contract provides a way for EVM code to interact with ERC20 standard interface that is interfacing directly with Polyjuice to control sUDT tokens on Layer 2 as if they were ERC20 tokens.

For security reason, developers should only use this [SudtERC20Proxy_UserDefinedDecimals bytecode](./SudtERC20Proxy_UserDefinedDecimals.bin) which code hash will be checked in `transfer_to_any_sudt` pre-compiled contract.

## Compile Solidity Contract in ethereum/solc:0.8.7 docker image
Here is the method that we compile SudtERC20Proxy_UserDefinedDecimals.sol.
```sh
> docker run --rm -v $(pwd):/contracts ethereum/solc:0.8.7 -o /contracts --bin --overwrite /contracts/SudtERC20Proxy_UserDefinedDecimals.sol

# checksum via ckb blake2b
> ckb-cli util blake2b --binary-path ERC20.bin 2>&1 | head -n1
0xa63fcc117d9c73fcaaf65bd469e70bcfe5b3c46f61d1e7e13761c969fd261316

# checksum via sha256sum
> sha256sum ERC20.bin 
9f7bf1ab25b377ddc339e6de79a800d4c7dc83de7e12057a0129b467794ce3a3  ERC20.bin
```

## Generate Code Hash

The content of `SudtERC20Proxy_UserDefinedDecimals.ContractCode.hex` is copied from running `test_cases::sudt_erc20_proxy::test_sudt_erc20_proxy_user_defined_decimals`.

```sh
# Generate the contract code hash of SudtERC20Proxy_UserDefinedDecimals
> ckb-cli util blake2b --binary-hex [the content string of SudtERC20Proxy_UserDefinedDecimals.ContractCode.hex]
0xde4542f5a5bd32c09cd98e9752281f88900a059aab7ac103edd9df214f136c52
```

The code hash above will be checked in `transfer_to_any_sudt` pre-compiled contract.
