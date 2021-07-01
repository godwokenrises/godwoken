# Migrate EVM contract to polyjuice

This documentation is aiming to provide a quick reference for migrating exist EVM contracts.

For high-level designs of polyjuice please check [known caveats of polyjuice](https://github.com/nervosnetwork/godwoken/blob/master/docs/known_caveats_of_polyjuice.md). We recommend you to read it first if you are not familiar with polyjuice.

## Migration guide

1. Please add [polyjuice web3 provider](https://github.com/RetricSu/polyjuice-providers-http) to your project according to which web3 library you are using.
2. Check the [EVM compatible documentation](https://github.com/nervosnetwork/godwoken-polyjuice/blob/main/docs/EVM-compatible.md).
3. Since in the polyjuice environment, there are not only Ethereum EOAs(Externally owned account), You must use `recover account` to replace the old `ecrecover` precompiled. See [polyjuice addition features](https://github.com/nervosnetwork/godwoken-polyjuice/blob/main/docs/Addition-Features.md) for details.

## Known Issue

some of this issues might be fixed soon, docs here will keep updating. please get back to check again.

1. `from` parameter in `eth_call` is optional, but due to an known issue, you should always set an valid `from` paramter when calling `eth_call`.
2. `eth_getBlockByNumber` is currently only support `latest` tag, other tags will not be recognised.


## Examples

* https://github.com/RetricSu/godwoken-polyjuice-compatibility-examples
