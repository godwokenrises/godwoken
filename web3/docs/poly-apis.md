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
      "web3Version": "1.12.0-rc2",
      "web3IndexerVersion": "1.12.0-rc2",
      "godwokenVersion": "1.12.0-rc1 4d0e922"
    },
    "fullnodeInfo": {
      "nodeMode": "fullnode",
      "rollupCell": {
        "typeHash": "0x4adf1f0e307f83227a58a16e861dae206a55a0baef8d2df7ea00b37aa032c50c",
        "typeScript": {
          "args": "0xe4d3b34a9ec6d38edae15ce992ab7240699668d46598658cb991415ab5112bb1",
          "code_hash": "0xbd8d100ab734e134e564bce85ea7d2318150e6baeabcba0a26514fa6cc4737b1",
          "hash_type": "type"
        }
      },
      "rollupConfig": {
        "chainId": "0x116e8",
        "challengeMaturityBlocks": "0x64",
        "finalityBlocks": "0x3",
        "requiredStakingCapacity": "0x2540be400",
        "rewardBurnRate": "0x32"
      },
      "gwScripts": {
        "deposit": {
          "script": {
            "args": "0x936f1538d66cfeea24e1283dc94b49c881afd20e1f9ebba31de5d252c669771d",
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type"
          },
          "typeHash": "0xd483176d9faa7278f8e05e14efd482a5eef36ec5abfb1b5a5d595d808a12579c"
        },
        "withdraw": {
          "script": {
            "args": "0x18e5cb4de6a634a0b3aa5630730e079ca1d6915c7d4fc92283bf8941a2da7a49",
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type"
          },
          "typeHash": "0xc4d713db311ab5df805675a1aa6a5f441fdf1fb2f24fc61c460cc157196cd173"
        },
        "stateValidator": {
          "script": {
            "args": "0x1536e8048d3305177c4044853034edec50d49799b1fba261ec9161bce7dcfc49",
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type"
          },
          "typeHash": "0xbd8d100ab734e134e564bce85ea7d2318150e6baeabcba0a26514fa6cc4737b1"
        },
        "stakeLock": {
          "script": {
            "args": "0xc1844e51890afaa05158fca36b56974b6a135fa94ac739c1076b7172c9855aae",
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type"
          },
          "typeHash": "0xeedb399ce44a66d73afcf5ab07e49248c357bcdf834f840da325aa032f369cb1"
        },
        "custodianLock": {
          "script": {
            "args": "0x18caa7562bf0c7f1135177ab4c767c80e01ae90675ba7fd8f3f2b87435c1c3f8",
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type"
          },
          "typeHash": "0xb67b195eaa601ccd97dc0768e87b0a9d66ad4f4db46bf858248bcf8811ec55be"
        },
        "challengeLock": {
          "script": {
            "args": "0xad24aba105ffd04aa24bb52969a7b80cf4f99d7d07f6822940c4ba572628b656",
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type"
          },
          "typeHash": "0xc4d713db311ab5df805675a1aa6a5f441fdf1fb2f24fc61c460cc157196cd173"
        },
        "l1Sudt": {
          "script": {
            "args": "0x57fdfd0617dcb74d1287bb78a7368a3a4bf9a790cfdcf5c1a105fd7cb406de0d",
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type"
          },
          "typeHash": "0x6283a479a3cf5d4276cd93594de9f1827ab9b55c7b05b3d28e4c2e0a696cfefd"
        },
        "l2Sudt": {
          "script": {
            "args": "0x27fb0835fc0505efca480a176dd68293c6774518126f6a2dc9f7fe818ae58a1e",
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type"
          },
          "typeHash": "0x7aefd62bb9d10281ba691fd933b96621ddd2ec2ce5fe11830713dc3918e75cb2"
        },
        "omniLock": {
          "script": {
            "args": "0x7005ea754481ea7d8fbd4f59d7c6d9dbe78b4437ca9dd532434e6ad1afa21d57",
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type"
          },
          "typeHash": "0xed4cbdbe1767275eab7c15664f2fcfa22980e235fd2fa4c83de06116f06eb50c"
        }
      },
      "eoaScripts": {
        "eth": {
          "typeHash": "0x45895f48b7cb8bb67c03b7ec4363215d01d23cf38c968ec97996782b44e12cbe",
          "script": {
            "args": "0x71609c8c54f368ac44972216a952439cfe42feefddfbca95fd205b609c2dc9a6",
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type"
          }
        }
      },
      "backends": {
        "sudt": {
          "validatorScriptTypeHash": "0x7aefd62bb9d10281ba691fd933b96621ddd2ec2ce5fe11830713dc3918e75cb2"
        },
        "meta": {
          "validatorScriptTypeHash": "0x2dadf58d141bbdec854136e3bb068191d6054ceaf4ac6cfc88ddc87cddb55222"
        },
        "polyjuice": {
          "validatorScriptTypeHash": "0xc24e643cd895b1ab2570d57d7447dc2d401ecea6ad1435eb380694292ce0cb15"
        },
        "ethAddrReg": {
          "validatorScriptTypeHash": "0x7bdd7121902e860a192ff9637f11e6605aee64001ad168d1c0b07ef3c5afbc3c"
        }
      },
      "accounts": {
        "polyjuiceCreator": {
          "id": "0x4",
          "scriptHash": "0xf22ec5de53b63396882c7bcb6d9bd1f7abc259f71202526a1eaf6c55d73f48fb"
        },
        "ethAddrReg": {
          "id": "0x2",
          "scriptHash": "0x1336e9e975e6618cd21c50eb7fc5607a8bb4599c7bdb453f3337d7d06d23b8a3"
        },
        "defaultFrom": {
          "id": "0x3",
          "scriptHash": "0x111e0520015ecea97cc20043ed71e55de6615b44f9b6217f2ffccdce33fe53d6"
        }
      },
      "chainId": "0x116e8",
      "gaslessTx": {
        "support": false
      }
    },
    "nodeInfo": {
      "nodeMode": "readonly",
      "rollupCell": {
        "typeHash": "0x4adf1f0e307f83227a58a16e861dae206a55a0baef8d2df7ea00b37aa032c50c",
        "typeScript": {
          "args": "0xe4d3b34a9ec6d38edae15ce992ab7240699668d46598658cb991415ab5112bb1",
          "code_hash": "0xbd8d100ab734e134e564bce85ea7d2318150e6baeabcba0a26514fa6cc4737b1",
          "hash_type": "type"
        }
      },
      "rollupConfig": {
        "chainId": "0x116e8",
        "challengeMaturityBlocks": "0x64",
        "finalityBlocks": "0x3",
        "requiredStakingCapacity": "0x2540be400",
        "rewardBurnRate": "0x32"
      },
      "gwScripts": {
        "deposit": {
          "script": {
            "args": "0x936f1538d66cfeea24e1283dc94b49c881afd20e1f9ebba31de5d252c669771d",
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type"
          },
          "typeHash": "0xd483176d9faa7278f8e05e14efd482a5eef36ec5abfb1b5a5d595d808a12579c"
        },
        "withdraw": {
          "script": {
            "args": "0x18e5cb4de6a634a0b3aa5630730e079ca1d6915c7d4fc92283bf8941a2da7a49",
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type"
          },
          "typeHash": "0xc4d713db311ab5df805675a1aa6a5f441fdf1fb2f24fc61c460cc157196cd173"
        },
        "stateValidator": {
          "script": {
            "args": "0x1536e8048d3305177c4044853034edec50d49799b1fba261ec9161bce7dcfc49",
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type"
          },
          "typeHash": "0xbd8d100ab734e134e564bce85ea7d2318150e6baeabcba0a26514fa6cc4737b1"
        },
        "stakeLock": {
          "script": {
            "args": "0xc1844e51890afaa05158fca36b56974b6a135fa94ac739c1076b7172c9855aae",
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type"
          },
          "typeHash": "0xeedb399ce44a66d73afcf5ab07e49248c357bcdf834f840da325aa032f369cb1"
        },
        "custodianLock": {
          "script": {
            "args": "0x18caa7562bf0c7f1135177ab4c767c80e01ae90675ba7fd8f3f2b87435c1c3f8",
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type"
          },
          "typeHash": "0xb67b195eaa601ccd97dc0768e87b0a9d66ad4f4db46bf858248bcf8811ec55be"
        },
        "challengeLock": {
          "script": {
            "args": "0xad24aba105ffd04aa24bb52969a7b80cf4f99d7d07f6822940c4ba572628b656",
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type"
          },
          "typeHash": "0xc4d713db311ab5df805675a1aa6a5f441fdf1fb2f24fc61c460cc157196cd173"
        },
        "l1Sudt": {
          "script": {
            "args": "0x57fdfd0617dcb74d1287bb78a7368a3a4bf9a790cfdcf5c1a105fd7cb406de0d",
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type"
          },
          "typeHash": "0x6283a479a3cf5d4276cd93594de9f1827ab9b55c7b05b3d28e4c2e0a696cfefd"
        },
        "l2Sudt": {
          "script": {
            "args": "0x27fb0835fc0505efca480a176dd68293c6774518126f6a2dc9f7fe818ae58a1e",
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type"
          },
          "typeHash": "0x7aefd62bb9d10281ba691fd933b96621ddd2ec2ce5fe11830713dc3918e75cb2"
        },
        "omniLock": {
          "script": {
            "args": "0x7005ea754481ea7d8fbd4f59d7c6d9dbe78b4437ca9dd532434e6ad1afa21d57",
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type"
          },
          "typeHash": "0xed4cbdbe1767275eab7c15664f2fcfa22980e235fd2fa4c83de06116f06eb50c"
        }
      },
      "eoaScripts": {
        "eth": {
          "typeHash": "0x45895f48b7cb8bb67c03b7ec4363215d01d23cf38c968ec97996782b44e12cbe",
          "script": {
            "args": "0x71609c8c54f368ac44972216a952439cfe42feefddfbca95fd205b609c2dc9a6",
            "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
            "hash_type": "type"
          }
        }
      },
      "backends": {
        "sudt": {
          "validatorScriptTypeHash": "0x7aefd62bb9d10281ba691fd933b96621ddd2ec2ce5fe11830713dc3918e75cb2"
        },
        "meta": {
          "validatorScriptTypeHash": "0x2dadf58d141bbdec854136e3bb068191d6054ceaf4ac6cfc88ddc87cddb55222"
        },
        "polyjuice": {
          "validatorScriptTypeHash": "0xc24e643cd895b1ab2570d57d7447dc2d401ecea6ad1435eb380694292ce0cb15"
        },
        "ethAddrReg": {
          "validatorScriptTypeHash": "0x7bdd7121902e860a192ff9637f11e6605aee64001ad168d1c0b07ef3c5afbc3c"
        }
      },
      "accounts": {
        "polyjuiceCreator": {
          "id": "0x4",
          "scriptHash": "0xf22ec5de53b63396882c7bcb6d9bd1f7abc259f71202526a1eaf6c55d73f48fb"
        },
        "ethAddrReg": {
          "id": "0x2",
          "scriptHash": "0x1336e9e975e6618cd21c50eb7fc5607a8bb4599c7bdb453f3337d7d06d23b8a3"
        },
        "defaultFrom": {
          "id": "0x3",
          "scriptHash": "0x111e0520015ecea97cc20043ed71e55de6615b44f9b6217f2ffccdce33fe53d6"
        }
      },
      "chainId": "0x116e8",
      "gaslessTx": {
        "support": false
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

*   `fullnodeInfo`: [`NodeInfo`](#type-nodeinfo)


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
