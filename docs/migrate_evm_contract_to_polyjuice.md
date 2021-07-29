# Migrate EVM contract to polyjuice

This documentation is aiming to provide a quick reference for migrating exist EVM contracts.

For high-level designs of polyjuice please check [known caveats of polyjuice](https://github.com/nervosnetwork/godwoken/blob/master/docs/known_caveats_of_polyjuice.md). We recommend you to read it first if you are not familiar with polyjuice.

## Migration guide

1. Please add [polyjuice web3 provider](https://github.com/nervosnetwork/polyjuice-provider) to your project according to which web3 library you are using.
2. Check the [EVM compatible documentation](https://github.com/nervosnetwork/godwoken-polyjuice/blob/main/docs/EVM-compatible.md).
3. Since in the polyjuice environment, there are not only Ethereum EOAs(Externally owned account), You must use `recover account` to replace the old `ecrecover` precompiled contract. See [polyjuice addition features](https://github.com/nervosnetwork/godwoken-polyjuice/blob/main/docs/Addition-Features.md) for details.
4. If you calculate create2 address on-chain, you need to call `eth_to_godwoken_addr` [precompiled contract](https://github.com/nervosnetwork/godwoken-polyjuice/blob/main/docs/Addition-Features.md#eth_to_godwoken_addr-spec) to convert it to a valid Godwoken address.

## Known Issue

Some of this issues might be fixed soon, docs here will keep updating. please get back to check again.

1. `from` parameter in `eth_call` is optional, but due to an known issue, you should always set an valid `from` paramter when calling `eth_call`.
2. calling a view function from smart-contract do not needs to pay fee in ethereum, however in godwoken we should set enough gasLimit and `gasPrice = 0`.
3. `eth_getBlockByNumber` is currently only support `latest` tag, other tags will not be recognised.


## Examples

* https://github.com/honestgoing/godwoken-polyjuice-compatibility-examples
