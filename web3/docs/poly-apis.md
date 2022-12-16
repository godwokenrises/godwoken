# Poly RPCs

## Table of Contents

- [Poly RPCs](#poly-rpcs)
  - [Table of Contents](#table-of-contents)
  - [RPC Methods](#rpc-methods)
      - [Method `poly_getCreatorId`](#method-poly_getcreatorid)
        - [Examples](#examples)
      - [Method `poly_getDefaultFromId`](#method-poly_getdefaultfromid)
        - [Examples](#examples-1)
      - [Method `poly_getContractValidatorTypeHash`](#method-poly_getcontractvalidatortypehash)
        - [Examples](#examples-2)
      - [Method `poly_getRollupTypeHash`](#method-poly_getrolluptypehash)
        - [Examples](#examples-3)
      - [Method `poly_getEthAccountLockHash`](#method-poly_getethaccountlockhash)
        - [Examples](#examples-4)
      - [Method `poly_version`](#method-poly_version)
        - [Examples](#examples-5)
      - [Method `poly_getEthTxHashByGwTxHash`](#method-poly_getethtxhashbygwtxhash)
        - [Examples](#examples-6)
      - [Method `poly_getGwTxHashByEthTxHash`](#method-poly_getgwtxhashbyethtxhash)
        - [Examples](#examples-7)
      - [Method `poly_getHealthStatus`](#method-poly_gethealthstatus)
        - [Examples](#examples-8)
  - [RPC Types](#rpc-types)
    - [Type `Uint32`](#type-uint32)
      - [Examples](#examples-9)
    - [Type `Uint64`](#type-uint64)
      - [Examples](#examples-10)
    - [Type `H256`](#type-h256)
      - [Examples](#examples-11)
    - [Type `PolyVersionInfo`](#type-polyversioninfo)
      - [Fields](#fields)
    - [Type `BackendInfo`](#type-backendinfo)
      - [Fields](#fields-1)
    - [Type `ScriptInfo`](#type-scriptinfo)
      - [Fields](#fields-2)
    - [Type `Script`](#type-script)
    - [Type `ScriptHashType`](#type-scripthashtype)
    - [Type `JsonBytes`](#type-jsonbytes)
      - [Example](#example)
    - [Type `Versions`](#type-versions)
      - [Examples](#examples-12)
      - [Fields](#fields-3)
    - [Type `RollupCell`](#type-rollupcell)
      - [Fields](#fields-4)
    - [Type `RollupConfig`](#type-rollupconfig)
      - [Fields](#fields-5)
    - [Type `NodeInfo`](#type-nodeinfo)
      - [Fields](#fields-6)
    - [Type `GwScripts`](#type-gwscripts)
      - [Fields](#fields-7)
    - [Type `EoaScripts`](#type-eoascripts)
      - [Fields](#fields-8)
    - [Type `Backends`](#type-backends)
      - [Fields](#fields-9)
    - [Type `AccountInfo`](#type-accountinfo)
      - [Fields](#fields-10)
    - [Type `Accounts`](#type-accounts)
      - [Fields](#fields-11)
    - [Type `HealthStatus`](#type-healthstatus)
      - [Fields](#fields-12)
    - [Type `GaslessTx`](#type-gaslesstx)
      - [Fields](#fields-13)

## RPC Methods

#### Method `poly_getCreatorId`
* `poly_getCreatorId()`
* result: [`Uint64`](#type-uint64)

Returns polyjuice creator account id

##### Examples

Request

```
{
  "jsonrpc": "2.0",
  "method": "poly_getCreatorId",
  "params": [],
  "id": 42
}
```

Response

```
{
  "jsonrpc": "2.0",
  "id": 42,
  "result": "0x4"
}
```

#### Method `poly_getDefaultFromId`
* `poly_getDefaultFromId()`
* result: [`Uint32`](#type-uint32)

Returns Web3 default from id for `eth_call` and `eth_estimateGas`

##### Examples

Request

```
{
  "jsonrpc": "2.0",
  "method": "poly_getDefaultFromId",
  "params": [],
  "id": 42
}
```

Response

```
{
  "jsonrpc": "2.0",
  "id": 42,
  "result": "0x3"
}
```

#### Method `poly_getContractValidatorTypeHash`
* `poly_getContractValidatorTypeHash()`
* result: [`H256`](#type-h256)

Returns Polyjuice contract validator script hash

##### Examples

Request

```
{
  "jsonrpc": "2.0",
  "method": "poly_getContractValidatorTypeHash",
  "params": [],
  "id": 42
}
```

Response

```
{
  "jsonrpc": "2.0",
  "id": 42,
  "result": "0x9b599c7df5d7b813f7f9542a5c8a0c12b65261a081b1dba02c2404802f772a15"
}
```

#### Method `poly_getRollupTypeHash`
* `poly_getRollupTypeHash()`
* result: [`H256`](#type-h256)

Returns Godwoken rollup script hash

##### Examples

Request

```
{
  "jsonrpc": "2.0",
  "method": "poly_getRollupTypeHash",
  "params": [],
  "id": 42
}
```

Response

```
{
  "jsonrpc": "2.0",
  "id": 42,
  "result": "0xe6d496b06df3c0ce45eed4eabddbb258e2f3dc7d268cc9952906ea61d33768a3"
}
```

#### Method `poly_getEthAccountLockHash`
* `poly_getEthAccountLockHash()`
* result: [`H256`](#type-h256)

Returns ETH account lock script hash

##### Examples

Request

```
{
  "jsonrpc": "2.0",
  "method": "poly_getEthAccountLockHash",
  "params": [],
  "id": 42
}
```

Response

```
{
  "jsonrpc": "2.0",
  "id": 42,
  "result": "0x1272c80507fe5e6cf33cf3e5da6a5f02430de40abb14410ea0459361bf74ebe0"
}
```

#### Method `poly_version`
* `poly_version()`
* result: [`PolyVersionInfo`](#type-polyversioninfo)

Returns node info for Godwoken & Polyjuice & Web3

##### Examples

Request

```
{
  "jsonrpc": "2.0",
  "method": "poly_version",
  "params": [],
  "id": 42
}
```

Response

```json
{
  "jsonrpc": "2.0",
  "id": 42,
  "result": {
    "versions": {
      "web3Version": "1.0.0-rc1",
      "web3IndexerVersion": "1.0.0-rc1",
      "godwokenVersion": "1.1.0 f3cdd47"
    },
    "nodeInfo": {
      "nodeMode": "fullnode",
      "rollupCell": {
        "typeHash": "0x7bbb4c2644595552a660c8f4fe1d5f84d6a670dc6bfd594bbd1a45516c3c7068",
        "typeScript": {
          "code_hash": "0x173eac817872c19a51470a47084108226beeace276212057ff962a37a4512dc6",
          "hash_type": "type",
          "args": "0x193704362146e0a8b1e64f62fd9f86add359581d5c2626c8408c00bac090cd9c"
        }
      },
      "rollupConfig": {
        "requiredStakingCapacity": "0x2540be400",
        "challengeMaturityBlocks": "0x64",
        "finalityBlocks": "0x3",
        "rewardBurnRate": "0x32",
        "chainId": "0x116e8"
      },
      "gwScripts": {
        "deposit": {
          "script": {
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type",
            "args": "0xe07e505668e03b34a7d941075cd48b9eca29b221dc3a8634e1ed7fd081c7a1e4"
          },
          "typeHash": "0xcf0bcea51b7478f06581743efa64bd706ce5f87424e430ed6ab5e681c62fb0fa"
        },
        "withdraw": {
          "script": {
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type",
            "args": "0xab449f20aa3235d01fd715467f1202346598e9d1ae816dad0979edeab59cd049"
          },
          "typeHash": "0x5722b1fa3d8ba814a9a59bcc05bdbd539f28569b4a2fb446ac08828911947542"
        },
        "stateValidator": {
          "script": {
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type",
            "args": "0xfc4484debef8af3d126dcb3805f46162ed1b624d9aed1d3f6fba243ff28ee26b"
          },
          "typeHash": "0x173eac817872c19a51470a47084108226beeace276212057ff962a37a4512dc6"
        },
        "stakeLock": {
          "script": {
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type",
            "args": "0x1e2b32501e4e75fb7d55443809e71df34026ce38039b81d5a6dc7c87672036f0"
          },
          "typeHash": "0x97b989de3fd83f28f0a35eebb61cab6f416fdc666f15ef6539f2b651fd2ca544"
        },
        "custodianLock": {
          "script": {
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type",
            "args": "0xe4e240a9fc8232f8200168ab7be230c108c04668e01fb15c35cd621a443f2dbe"
          },
          "typeHash": "0xdef2218cdcda1c9b77c2a1c54dd6635eedccea507dbe5f377f8a1981d6bb6256"
        },
        "challengeLock": {
          "script": {
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type",
            "args": "0x3d73bc2b055ffe642016be69019889bbeecf3876e2f37ae7d336326823a84452"
          },
          "typeHash": "0x5722b1fa3d8ba814a9a59bcc05bdbd539f28569b4a2fb446ac08828911947542"
        },
        "l1Sudt": {
          "script": {
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type",
            "args": "0x57fdfd0617dcb74d1287bb78a7368a3a4bf9a790cfdcf5c1a105fd7cb406de0d"
          },
          "typeHash": "0x6283a479a3cf5d4276cd93594de9f1827ab9b55c7b05b3d28e4c2e0a696cfefd"
        },
        "l2Sudt": {
          "script": {
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type",
            "args": "0x9cb87271452ef0d91bf0ed4d590c12e1c50ea81aebcd763a60a57fc8ac471fea"
          },
          "typeHash": "0x6432713bd4bb2c22eca1b8e962d712e8eccc2a740f3b5433848414591ea26fa7"
        },
        "omniLock": {
          "script": {
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type",
            "args": "0xa57c8644b456c90d43cd915c5fdc15b998a320b2964033f9639771dc32df35f0"
          },
          "typeHash": "0x8adcbae4e6f4fc21977c328965d4740cb9de91b4277920a17839aeefe9e2795a"
        }
      },
      "eoaScripts": {
        "eth": {
          "typeHash": "0xc9b8427bee1b37f863f18562e48b8396d92733238a82fd978d3d63a911307ef8",
          "script": {
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type",
            "args": "0x895aeebbb4a3069fb2dbecfb20b56892f3da9f49aeae68a455be2b90909f0393"
          }
        }
      },
      "backends": {
        "sudt": {
          "validatorCodeHash": "0xb9d9375c0fd4d50ed95019d8307961238316cd18c1fb3faeb15ac0d3c6d76bda",
          "generatorCodeHash": "0xf87824d6723b3c0be51b1213e8b35a6e8587a10f2f27734a344f201bf2ab05ef",
          "validatorScriptTypeHash": "0x6432713bd4bb2c22eca1b8e962d712e8eccc2a740f3b5433848414591ea26fa7"
        },
        "meta": {
          "validatorCodeHash": "0x4cf8b2b8b04dab0de276093de71f98592a5d683d42e2aa70110e904b564fc1c3",
          "generatorCodeHash": "0x8c6c44b97d9de23dc0047356fb0b3e258a60e14a1f2bfa8f95ddc7b41985a8e0",
          "validatorScriptTypeHash": "0x0b252876f97129e2564a8751d0c18ec73f9b93a52ce5a60ffb455cb74c678e1b"
        },
        "polyjuice": {
          "validatorCodeHash": "0xb94f8adecaa8638318fc62f609431daa225bc22143ce23c03c59c78a78653448",
          "generatorCodeHash": "0x9c64de23b69dc8496879d18156f6e79fa7cae4a9faf67e23e0ab3e7d1687ac35",
          "validatorScriptTypeHash": "0x3fefe20277a6e6125b253ee31c060207b2460262669628aeb16d0a337b678236"
        },
        "ethAddrReg": {
          "validatorCodeHash": "0x9085bd6a550a9921b46d23ba7d9b0f9f5c5d0c9c00999988cd907ce16015e467",
          "generatorCodeHash": "0xe2ba730569850cca7a56c9a96754bd0bfd784c8f001e997e4512edf572190c4a",
          "validatorScriptTypeHash": "0x0af018a61a1d0aaa749603f250bd59ba1712a949089552cfa10887e0dd2fa6ee"
        }
      },
      "accounts": {
        "polyjuiceCreator": {
          "id": "0x4",
          "scriptHash": "0xb41fbe158237d2f70f0e3d006b2a5dcd804fcfade6fea9a345091794658269f6"
        },
        "ethAddrReg": {
          "id": "0x2",
          "scriptHash": "0xfb9c975120aa3545d00ddeb09b114eb4319479afa27aef5ed22089f2dff0423d"
        },
        "defaultFrom": {
          "id": "0x3",
          "scriptHash": "0xffe2e575a9c327f160e09d142bf21bcedbf79f23d585be3b87dacde843e171a4"
        }
      },
      "chainId": "0x116e8",
      "gaslessTx": {
        "support": true,
        "entrypointAddress": "0x954dcfc2b81446bc83254c1fa36a037613bd2481"
      }
    }
  }
}
```

#### Method `poly_getEthTxHashByGwTxHash`
* `poly_getEthTxHashByGwTxHash()`
* result: [`H256`](#type-h256)

Get eth_tx_hash by gw_tx_hash

##### Examples

Request

```json
{
  "jsonrpc": "2.0",
  "method": "poly_getEthTxHashByGwTxHash",
  "params": ["0xa872560e2e7d2ca9bdefdae1810a0e01c5597227137c8862a573d1f4738aa360"],
  "id": 42
}
```

Response

```json
{
  "jsonrpc": "2.0",
  "id": 42,
  "result": "0x18dba16296dd00ae9b9339d62c2aac60fff09389dd9d3e8ad431357cc0180dfd"
}
```

#### Method `poly_getGwTxHashByEthTxHash`
* `poly_getGwTxHashByEthTxHash()`
* result: [`H256`](#type-h256)

Get gw_tx_hash by eth_tx_hash

##### Examples

Request

```json
{
  "jsonrpc": "2.0",
  "method": "poly_getGwTxHashByEthTxHash",
  "params": ["0x18dba16296dd00ae9b9339d62c2aac60fff09389dd9d3e8ad431357cc0180dfd"],
  "id": 42
}
```

Response

```json
{
  "jsonrpc": "2.0",
  "id": 42,
  "result": "0xa872560e2e7d2ca9bdefdae1810a0e01c5597227137c8862a573d1f4738aa360"
}
```

#### Method `poly_getHealthStatus`
* `poly_getHealthStatus()`
* result: [`HealthStatus`](#type-healthstatus)

Get web3 server health status

##### Examples

Request

```json
{
    "id": 2,
    "jsonrpc": "2.0",
    "method": "poly_getHealthStatus",
    "params": []
}
```

Response

```json
{
    "jsonrpc": "2.0",
    "id": 2,
    "result": {
        "status": true,
        "pingNode": "pong",
        "pingFullNode": "pong",
        "pingRedis": "PONG",
        "isDBConnected": true,
        "syncBlocksDiff": 0,
        "ckbOraclePrice": "0.00408"
    }
}
```

## RPC Types

### Type `Uint32`

The  32-bit unsigned integer type encoded as the 0x-prefixed hex string in JSON.

#### Examples

|  JSON | Decimal Value |
| --- |--- |
|  “0x0” | 0 |
|  “0x10” | 16 |
|  “10” | Invalid, 0x is required |

### Type `Uint64`

The  64-bit unsigned integer type encoded as the 0x-prefixed hex string in JSON.

#### Examples

|  JSON | Decimal Value |
| --- |--- |
|  “0x0” | 0 |
|  “0x10” | 16 |
|  “10” | Invalid, 0x is required |

### Type `H256`

The 32-byte fixed-length binary data.

The name comes from the number of bits in the data.

In JSONRPC, it is encoded as a 0x-prefixed hex string.

#### Examples

```
0x696447c51fdb84d0e59850b26bc431425a74daaac070f2b14f5602fbb469912a
```


### Type `PolyVersionInfo`


#### Fields

`PolyVersionInfo` is a JSON object with the following fields.

*   `versions`: [`Versions`](#type-versions)

*   `nodeInfo`: [`NodeInfo`](#type-nodeinfo)


### Type `BackendInfo`


#### Fields

`BackendInfo` is a JSON object with the following fields.

*   `validatorCodeHash`: [`H256`](#type-h256) - Validator script's code hash

*   `generatorCodeHash`: [`H256`](#type-h256) - Generator script's code hash

*   `validatorScriptTypeHash`: [`H256`](#type-h256) - Validate script's hash (script.hash_type = type)


### Type `ScriptInfo`


#### Fields

`ScriptInfo` is a JSON object with the following fields.

*   `typeHash`: [`H256`](#type-h256) - Hash of script

*   `script`: [`Script`](#type-script) - Script's deploy info


### Type `Script`

More info [CKB RPC](https://github.com/nervosnetwork/ckb/blob/develop/rpc/README.md#type-script)

### Type `ScriptHashType`

More info [CKB RPC](https://github.com/nervosnetwork/ckb/blob/develop/rpc/README.md#type-scripthashtype)

### Type `JsonBytes`

Variable-length binary encoded as a 0x-prefixed hex string in JSON.

#### Example

|  JSON | Binary |
| --- |--- |
|  “0x” | Empty binary |
|  “0x00” | Single byte 0 |
|  “0x636b62” | 3 bytes, UTF-8 encoding of ckb |
|  “00” | Invalid, 0x is required |
|  “0x0” | Invalid, each byte requires 2 digits |

### Type `Versions`

Version info of Godwoken & Web3 & Web3 Indexer.

#### Examples


```
{
  "web3Version": "1.0.0-rc1",
  "web3IndexerVersion": "1.0.0-rc1",
  "godwokenVersion": "1.1.0 f3cdd47"
}
```


#### Fields

`Versions` is a JSON object with the following fields.

*   `web3Version`: `string` - Version of Godwoken Web3

*   `web3IndexerVersion`: `string` - Version of Godwoken Web3 Indexer

*   `godwokenVersion`: `string` - Version of Godwoken


### Type `RollupCell`

#### Fields

`RollupCell` is a JSON object with the following fields.

*   `typeHash`: [`H256`](#type-h256) - Hash of typeScript

*   `typeScript`: [`Script`](#type-script) - Rollup script info


### Type `RollupConfig`

#### Fields

`RollupConfig` is a JSON object with the following fields.

*   `requiredStakingCapacity`: [`Uint64`](#type-uint64) - The minimal capacity required for staking to be the chain generator

*   `challengeMaturityBlocks`: [`Uint64`](#type-uint64) - Challenge maturity blocks

*   `finalityBlocks`: [`Uint64`](#type-uint64) - Finality Blocks

*   `chainId`: [`Uint64`](#type-uint64) - Chain ID, more info: [EIP155](https://eips.ethereum.org/EIPS/eip-155)



### Type `NodeInfo`

Info of Godwoken & Web3 node.

#### Fields

`NodeInfo` is a JSON object with the following fields.

*   `nodeMode`: `fullnode | readonly` - fullnode or readonly node

*   `rollupCell`: [`RollupCell`](#type-rollupcell) - Rollup cell info

*   `rollupConfig`: [`RollupConfig`](#type-rollupconfig) - Rollup config info

*   `gwScripts`: [`GwScripts`](#type-gwscripts) - Godwoken scripts deploy info

*   `eoaScripts`: [`EoaScripts`](#type-eoaScripts) - Supported EOA scripts

*   `backends`: [`Backends`](#type-backends)

*   `accounts`: [`Accounts`](#type-accounts) 

*   `chainId`: [`Uint64`](#type-uint64) - Chain ID, more info: [EIP155](https://eips.ethereum.org/EIPS/eip-155)

*   `gaslessTx`: [`GaslessTx`](#type-gaslessTx) - Gasless Tx feature, more info: [additional feature](/docs/addtional-feature.md#gasless-transaction)

### Type `GwScripts`

Godwoken scripts deploy info

#### Fields

`GwScripts` is a JSON object with the following fields.

*   `deposit`: [`ScriptInfo`](#type-scriptinfo) - Deposit lock script

*   `withdraw`: [`ScriptInfo`](#type-scriptinfo) - Withdrawal lock script

*   `stateValidator`: [`ScriptInfo`](#type-scriptinfo) - State validator script

*   `stakeLock`: [`ScriptInfo`](#type-scriptinfo) - State lock script

*   `custodianLock`: [`ScriptInfo`](#type-scriptinfo) - Custodian lock script

*   `challengeLock`: [`ScriptInfo`](#type-scriptinfo) - Challenge lock script

*   `l1Sudt`: [`ScriptInfo`](#type-scriptinfo) - L1 sudt script

*   `l2Sudt`: [`ScriptInfo`](#type-scriptinfo) - L2 sudt script

*   `omniLock`: [`ScriptInfo`](#type-scriptinfo) - Omni lock script

*   `challengeLock`: [`ScriptInfo`](#type-scriptinfo) - Challenge lock script


### Type `EoaScripts`

Supported lock scripts for EOA accounts

#### Fields

`EoaScripts` is a JSON object with the following fields.

*   `eth`: [`ScriptInfo`](#type-scriptinfo) - Ethereum

### Type `Backends`


#### Fields

`Backends` is a JSON object with the following fields.

*   `sudt`: [`BackendInfo`](#type-backendinfo) - Sudt contract backend info

*   `meta`: [`BackendInfo`](#type-backendinfo) - Meta contract backend info

*   `polyjuice`: [`BackendInfo`](#type-backendinfo) - Polyjuice backend info

*   `ethAddrReg`: [`BackendInfo`](#type-backendinfo) - Eth Address Registry contract backend info


### Type `AccountInfo`

Some helpful system accounts.

#### Fields

`AccountInfo` is a JSON object with the following fields.

*   `id`: [`Uint32`](#type-uint32) - Godwoken's account id

*   `scriptHash`: [`H256`](#type-h256) - Godwoken's account script hash


### Type `Accounts`

Describes the accounts web3 used.

#### Fields

`Accounts` is a JSON object with the following fields.

*   `polyjuiceCreator`: [`AccountInfo`](#type-accountinfo) - Polyjuice creator account

*   `ethAddrReg`: [`AccountInfo`](#type-accountinfo) - Godwoken builtin eth address mapping registry account

*   `defaultFrom`: [`AccountInfo`](#type-accountinfo) - Default from account used in `eth_call` and `eth_estimateGas`

### Type `HealthStatus`

Describes the web3 server health status.

#### Fields

*   `status`: `boolean` - Health status, should be true

*   `pingNode`: `string` - Godwoken readonly node ping result, should be "pong"

*   `pingFullNode`: `string` - Godwoken fullnode node ping result, should be "pong"

*   `pingRedis`: `string` - Redis server ping result, should be "PONG"

*   `isDBConnected`: `boolean` - Database connection status, should be true

*   `syncBlocksDiff`: `number` - Web3 sync behind godwoken blocks count, eg 2 means sync behind 2 blocks, 0 means sync to the latest

*   `ckbOraclePrice`: `string` - CKBPriceOracle updating value or "PriceOracleNotEnabled" if it is turned off, should not be null

### Type `GaslessTx`

Describes the accounts web3 used.

#### Fields

`GaslessTx` is a JSON object with the following fields.

*   `support`: `boolean` - Weather the feature is turned on or not

*   `entrypointAddress`: `string` - the entrypoint contract account address
