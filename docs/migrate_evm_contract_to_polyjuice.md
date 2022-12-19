# Migrate EVM contracts to polyjuice

This documentation aims to provide a quick reference for migrating existing EVM contracts.

For high-level designs of polyjuice please check [known caveats of polyjuice](https://github.com/nervosnetwork/godwoken/blob/master/docs/known_caveats_of_polyjuice.md). We recommend you read this first if you need to become more familiar with polyjuice.

## Migration guide

1. Please add [polyjuice web3 provider](https://github.com/nervosnetwork/polyjuice-provider) to your project according to the web3 library you are using.
2. Check the [EVM compatible documentation](https://github.com/nervosnetwork/godwoken-polyjuice/blob/main/docs/EVM-compatible.md).
3. Since in the polyjuice environment, there are more than only Ethereum EOAs(Externally owned account), You must use `recover account` to replace the old `ecrecover` precompiled contract. See [polyjuice addition features](https://github.com/nervosnetwork/godwoken-polyjuice/blob/main/docs/Addition-Features.md) for more details.
4. If you calculate create2 address on-chain, you need to call `eth_to_godwoken_addr` [precompiled contract](https://github.com/nervosnetwork/godwoken-polyjuice/blob/main/docs/Addition-Features.md#eth_to_godwoken_addr-spec) to convert it to a valid Godwoken address.

## Known Issue

Some of these issues will be fixed soon. Docs are constantly being updated. So, make sure to check again. 

1. `from` parameter in `eth_call` is optional, but due to a known issue, you should always set a valid `from` parameter when calling `eth_call`.
2. calling a view function from a smart-contract does not need to pay fees on Ethereum. however, with Godwoken we should set enough gasLimit and `gasPrice = 0`.
3. `eth_getBlockByNumber` is currently only support `latest` tag, other tags will not be recognised.


## Examples

* https://github.com/honestgoing/godwoken-polyjuice-compatibility-examples
