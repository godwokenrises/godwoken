# RPC

## Table of Contents

* [RPC Methods](#rpc-methods)
    * [Method `gw_ping`](#method-gw_ping)
    * [Method `gw_get_tip_block_hash`](#method-gw_get_tip_block_hash)
    * [Method `gw_get_block_hash`](#method-gw_get_block_hash)
    * [Method `gw_get_block`](#method-gw_get_block)
    * [Method `gw_get_block_by_number`](#method-gw_get_block_by_number)
    * [Method `gw_get_block_committed_info`](#method-gw_get_block_committed_info)
    * [Method `gw_get_balance`](#method-gw_get_balance)
    * [Method `gw_get_storage_at`](#method-gw_get_storage_at)
    * [Method `gw_get_account_id_by_script_hash`](#method-gw_get_account_id_by_script_hash)
    * [Method `gw_get_nonce`](#method-gw_get_nonce)
    * [Method `gw_get_script`](#method-gw_get_script)
    * [Method `gw_get_script_hash`](#method-gw_get_script_hash)
    * [Method `gw_get_script_hash_by_registry_address`](#method-gw_get_script_hash_by_registry_address)
    * [Method `gw_get_registry_address_by_script_hash`](#method-gw_get_registry_address_by_script_hash)
    * [Method `gw_get_data`](#method-gw_get_data)
    * [Method `gw_get_transaction`](#method-gw_get_transaction)
    * [Method `gw_get_transaction_receipt`](#method-gw_get_transaction_receipt)
    * [Method `gw_get_withdrawal`](#method-gw_get_withdrawal)
    * [Method `gw_execute_l2transaction`](#method-gw_execute_l2transaction)
    * [Method `gw_execute_raw_l2transaction`](#method-gw_execute_raw_l2transaction)
    * [Method `gw_compute_l2_sudt_script_hash`](#method-gw_compute_l2_sudt_script_hash)
    * [Method `gw_get_fee_config`](#method-gw_get_fee_config)
    * [Method `gw_get_mem_pool_state_root`](#method-gw_get_mem_pool_state_root)
    * [Method `gw_get_mem_pool_state_ready`](#method-gw_get_mem_pool_state_ready)
    * [Method `gw_get_node_info`](#method-gw_get_node_info)
    * [Method `gw_reload_config`](#method-gw_reload_config)
    * [Method `gw_submit_l2transaction`](#method-gw_submit_l2transaction)
    * [Method `gw_submit_withdrawal_request`](#method-gw_submit_withdrawal_request)
    * [Method `gw_get_last_submitted_info`](#method-gw_get_last_submitted_info)
* [RPC Types](#rpc-types)
    * [Type `Uint32`](#type-uint32)
    * [Type `Uint64`](#type-uint64)
    * [Type `Uint128`](#type-uint128)
    * [Type `Uint256`](#type-uint256)
    * [Type `H256`](#type-h256)
    * [Type `JsonBytes`](#type-jsonbytes)
    * [Type `Backend`](#type-backend)
    * [Type `NodeInfo`](#type-nodeinfo)
    * [Type `EoaScript`](#type-eoascript)
    * [Type `GwScript`](#type-gwscript)
    * [Type `RollupCell`](#type-rollupcell)
    * [Type `NodeRollupConfig`](#type-noderollupconfig)
    * [Type `L2BlockWithStatus`](#type-l2block)
    * [Type `L2Block`](#type-l2block)
    * [Type `KVPair`](#type-kvpair)
    * [Type `RawL2Block`](#type-rawl2block)
    * [Type `AccountMerkleState`](#type-accountinfo)
    * [Type `SubmitTransaction`](#type-submittransaction)
    * [Type `SubmitWithdrawal`](#type-submitwithdrawal)
    * [Type `L2TransactionWithStatus`](#type-l2transactionwithstatus)
    * [Type `L2Transaction`](#type-l2transaction)
    * [Type `RawL2Transaction`](#type-rawl2transaction)
    * [Type `L2TransactionReceipt`](#type-l2transactionreceipt)
    * [Type `WithdrawalWithStatus`](#type-withdrawalwithstatus)
    * [Type `WithdrawalRequestExtra`](#type-withdrawalrequestextra)
    * [Type `WithdrawalRequest`](#type-withdrawalrequest)
    * [Type `RawWithdrawalRequest`](#type-rawwithdrawalrequest)
    * [Type `L2BlockCommittedInfo`](#type-l2blockcommittedinfo)
    * [Type `LogItem`](#type-logitem)
    * [Type `RunResult`](#type-runresult)
    * [Type `FeeConfig`](#type-feeconfig)
    * [Type `LastL2BlockCommittedInfo`](#type-lastl2blockcommittedinfo)
    * [Type `RegistryAddress`](#type-registryaddress)
    * [Type `SerializedRegistryAddress`](#type-serializedregistryaddress)
    * [Type `SerializedL2Transaction`](#type-serializedmoleculeschema)
    * [Type `SerializedRawL2Transaction`](#type-serializedmoleculeschema)
    * [Type `SerializedWithdrawalRequest`](#type-serializedmoleculeschema)
    * [Type `Script`](#type-script)
    * [Type `ScriptHashType`](#type-scripthashtype)
    

## Methods

### Method `gw_ping`
* `gw_ping()`
* result: `pong`

Get node info.

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_ping",
    "params": []
}
```

Response

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "result": "pong"
}
```

### Method `gw_get_node_info`
* params: None
* result: [`NodeInfo`](#type-nodeinfo)

Get node info.

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_get_node_info",
    "params": []
}
```

Response

``` json
{
    "jsonrpc": "2.0",
    "id": 42,
    "result": {
        "mode": "fullnode",
        "version": "1.1.0 f3cdd47",
        "backends": [
            {
                "validator_code_hash": "0xb9d9375c0fd4d50ed95019d8307961238316cd18c1fb3faeb15ac0d3c6d76bda",
                "generator_code_hash": "0xf87824d6723b3c0be51b1213e8b35a6e8587a10f2f27734a344f201bf2ab05ef",
                "validator_script_type_hash": "0xb6176a6170ea33f8468d61f934c45c57d29cdc775bcd3ecaaec183f04b9f33d9",
                "backend_type": "sudt"
            },
            {
                "validator_code_hash": "0xb94f8adecaa8638318fc62f609431daa225bc22143ce23c03c59c78a78653448",
                "generator_code_hash": "0x9c64de23b69dc8496879d18156f6e79fa7cae4a9faf67e23e0ab3e7d1687ac35",
                "validator_script_type_hash": "0x1629b04b49ded9e5747481f985b11cba6cdd4ffc167971a585e96729455ca736",
                "backend_type": "polyjuice"
            },
            {
                "validator_code_hash": "0x9085bd6a550a9921b46d23ba7d9b0f9f5c5d0c9c00999988cd907ce16015e467",
                "generator_code_hash": "0xe2ba730569850cca7a56c9a96754bd0bfd784c8f001e997e4512edf572190c4a",
                "validator_script_type_hash": "0xa30dcbb83ebe571f49122d8d1ce4537679ebf511261c8ffaaa6679bf9fdea3a4",
                "backend_type": "eth_addr_reg"
            },
            {
                "validator_code_hash": "0x4cf8b2b8b04dab0de276093de71f98592a5d683d42e2aa70110e904b564fc1c3",
                "generator_code_hash": "0x8c6c44b97d9de23dc0047356fb0b3e258a60e14a1f2bfa8f95ddc7b41985a8e0",
                "validator_script_type_hash": "0x37b25df86ca495856af98dff506e49f2380d673b0874e13d29f7197712d735e8",
                "backend_type": "meta"
            }
        ],
        "eoa_scripts": [
            {
                "type_hash": "0x07521d0aa8e66ef441ebc31204d86bb23fc83e9edc58c19dbb1b0ebe64336ec0",
                "script": {
                    "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
                    "hash_type": "type",
                    "args": "0x66056785e4e989729053508c30d620ead06b377f600eedc0419e6858e459ccfa"
                },
                "eoa_type": "eth"
            }
        ],
        "gw_scripts": [
            {
                "type_hash": "0x1e44736436b406f8e48a30dfbddcf044feb0c9eebfe63b0f81cb5bb727d84854",
                "script": {
                    "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
                    "hash_type": "type",
                    "args": "0x063555cc66a1c270aafbe9324718232289a462f4d9edfc7a57f9c6e0f8257669"
                },
                "script_type": "state_validator"
            },
            {
                "type_hash": "0x50704b84ecb4c4b12b43c7acb260ddd69171c21b4c0ba15f3c469b7d143f6f18",
                "script": {
                    "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
                    "hash_type": "type",
                    "args": "0x86d24e5cb132478005dcf2b59680a9f37011cb54a5947f42f19ba5076bc6594d"
                },
                "script_type": "deposit"
            },
            {
                "type_hash": "0x06ae0706bb2d7997d66224741d3ec7c173dbb2854a6d2cf97088796b677269c6",
                "script": {
                    "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
                    "hash_type": "type",
                    "args": "0xbfef6580c1f93b98fa7d33bb3faa63255caba9bfbebfbada5eab4ce195052b9f"
                },
                "script_type": "withdraw"
            },
            {
                "type_hash": "0x7f5a09b8bd0e85bcf2ccad96411ccba2f289748a1c16900b0635c2ed9126f288",
                "script": {
                    "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
                    "hash_type": "type",
                    "args": "0x0fc0f22f9a6e000692159c9d5dc633fba7ffcd1f1f2218d23aa2ede96f4b471d"
                },
                "script_type": "stake_lock"
            },
            {
                "type_hash": "0x85ae4db0dd83f428a31deb342e4000af37ce2c9645d9e619df00096e3c50a2bb",
                "script": {
                    "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
                    "hash_type": "type",
                    "args": "0xc4695745c69c298c89bc701b6cc8614332c6fd8a2ed160e04748fc6fda636e71"
                },
                "script_type": "custodian_lock"
            },
            {
                "type_hash": "0x06ae0706bb2d7997d66224741d3ec7c173dbb2854a6d2cf97088796b677269c6",
                "script": {
                    "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
                    "hash_type": "type",
                    "args": "0x7997689a9038a5487535cd8297d37b39840e140c849efd6f07ecc20ee9b9c244"
                },
                "script_type": "challenge_lock"
            },
            {
                "type_hash": "0xc5e5dcf215925f7ef4dfaf5f4b4f105bc321c02776d6e7d52a1db3fcd9d011a4",
                "script": {
                    "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
                    "hash_type": "type",
                    "args": "0x4db75e03349f4f2ec792476035dd1b7376c683130f7e2e74024be2d9ee064511"
                },
                "script_type": "l1_sudt"
            },
            {
                "type_hash": "0xb6176a6170ea33f8468d61f934c45c57d29cdc775bcd3ecaaec183f04b9f33d9",
                "script": {
                    "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
                    "hash_type": "type",
                    "args": "0xe9374fd920cd4144ce72ab7ef3405d89e5f8530d586ba986e993f1d285060a7a"
                },
                "script_type": "l2_sudt"
            },
            {
                "type_hash": "0x79f90bb5e892d80dd213439eeab551120eb417678824f282b4ffb5f21bad2e1e",
                "script": {
                    "code_hash": "0x00000000000000000000000000000000000000000000000000545950455f4944",
                    "hash_type": "type",
                    "args": "0x1b8572b16c07f46a0efed623aea6de05d45985b9a7c1b0b52276da5d9f9615b7"
                },
                "script_type": "omni_lock"
            }
        ],
        "rollup_cell": {
            "type_hash": "0x702359ea7f073558921eb50d8c1c77e92f760c8f8656bde4995f26b8963e2dd8",
            "type_script": {
                "code_hash": "0x1e44736436b406f8e48a30dfbddcf044feb0c9eebfe63b0f81cb5bb727d84854",
                "hash_type": "type",
                "args": "0x86c7429247beba7ddd6e4361bcdfc0510b0b644131e2afb7e486375249a01802"
            }
        },
        "rollup_config": {
            "required_staking_capacity": "0x3691d6afc000",
            "challenge_maturity_blocks": "0x1c2",
            "finality_blocks": "0x64",
            "reward_burn_rate": "0x32",
            "chain_id": "0x116e9"
        }
    }
}
```

### Method `gw_get_tip_block_hash`
* params: None
* result: [`H256`](#type-h256)

Get hash of the tip block.

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_get_tip_block_hash",
    "params": []
}
```

Response

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "result": "0xcbe210ac82461388300cf62197062374ef88160a2755c95fab3e1a4a686aa372"
}
```

### Method `gw_get_block_hash`
* params:
    * `block_number`: [`Uint64`](#type-uint64)
* result: [`H256`](#type-h256) `|` `null`

Get block hash by number.


#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_get_block_hash",
    "params": ["0x2a"]
}
```

Response

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "result": "0xbf55ed82cf4b33a83df679b6cba8444a3527b64735d5b5c73f6163c24af525aa"
}
```

### Method `gw_get_block`
* params:
    * `block_hash`: [`H256`](#type-h256)
* result: [`L2BlockWithStatus`](#type-h256) `|` `null`

Get block.

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_get_block",
    "params": ["0x4ac339b063e52dac1b845d935788f379ebcdb0e33ecce077519f39929dbc8829"]
}
```

Response

``` json
{
    "jsonrpc": "2.0",
    "id": 42,
    "result": {
        "block": {
            "raw": {
                "number": "0x1",
                "parent_block_hash": "0x61bcff6f20e8be09bbe8e36092a9cc05dd3fa67e3841e206e8c30ae0dd7032df",
                "block_producer": "0x0200000014000000715ab282b873b79a7be8b0e8c13c4e8966a52040",
                "stake_cell_owner_lock_hash": "0xf245705db4fe72be953e4f9ee3808a1700a578341aa80a8b2349c236c4af64e5",
                "timestamp": "0x180a1e9f622",
                "prev_account": {
                    "merkle_root": "0x52baafb94a6b1c43e7361460e3bb926ca6a7ab874cec19ba71a1a5dea501c34f",
                    "count": "0x3"
                },
                "post_account": {
                    "merkle_root": "0x52baafb94a6b1c43e7361460e3bb926ca6a7ab874cec19ba71a1a5dea501c34f",
                    "count": "0x3"
                },
                "submit_transactions": {
                    "tx_witness_root": "0x0000000000000000000000000000000000000000000000000000000000000000",
                    "tx_count": "0x0",
                    "prev_state_checkpoint": "0xe4c6f7d8dc63058ed833552954f8e1635bdaa9608866dc3eaa26b148de503ba9"
                },
                "submit_withdrawals": {
                    "withdrawal_witness_root": "0x0000000000000000000000000000000000000000000000000000000000000000",
                    "withdrawal_count": "0x0"
                },
                "state_checkpoint_list": []
            },
            "kv_state": [],
            "kv_state_proof": "0x",
            "transactions": [],
            "block_proof": "0x4c5061bcff6f20e8be09bbe8e36092a9cc05dd3fa67e3841e206e8c30ae0dd7032df4fff",
            "withdrawal_requests": [],
            "hash": "0x4ac339b063e52dac1b845d935788f379ebcdb0e33ecce077519f39929dbc8829"
        },
        "status": "finalized"
    }
}
```

### Method `gw_get_block_by_number`
* params:
    * `block_number`: [`Uint64`](#type-uint64)
* result: [`L2Block`](#type-h256) `|` `null`

Get block by number.

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_get_block_by_number",
    "params": ["0x1"]
}
```

Response

``` json
{
    "jsonrpc": "2.0",
    "id": 42,
    "result": {
        "raw": {
            "number": "0x1",
            "parent_block_hash": "0x61bcff6f20e8be09bbe8e36092a9cc05dd3fa67e3841e206e8c30ae0dd7032df",
            "block_producer": "0x0200000014000000715ab282b873b79a7be8b0e8c13c4e8966a52040",
            "stake_cell_owner_lock_hash": "0xf245705db4fe72be953e4f9ee3808a1700a578341aa80a8b2349c236c4af64e5",
            "timestamp": "0x180a1e9f622",
            "prev_account": {
                "merkle_root": "0x52baafb94a6b1c43e7361460e3bb926ca6a7ab874cec19ba71a1a5dea501c34f",
                "count": "0x3"
            },
            "post_account": {
                "merkle_root": "0x52baafb94a6b1c43e7361460e3bb926ca6a7ab874cec19ba71a1a5dea501c34f",
                "count": "0x3"
            },
            "submit_transactions": {
                "tx_witness_root": "0x0000000000000000000000000000000000000000000000000000000000000000",
                "tx_count": "0x0",
                "prev_state_checkpoint": "0xe4c6f7d8dc63058ed833552954f8e1635bdaa9608866dc3eaa26b148de503ba9"
            },
            "submit_withdrawals": {
                "withdrawal_witness_root": "0x0000000000000000000000000000000000000000000000000000000000000000",
                "withdrawal_count": "0x0"
            },
            "state_checkpoint_list": []
        },
        "kv_state": [],
        "kv_state_proof": "0x",
        "transactions": [],
        "block_proof": "0x4c5061bcff6f20e8be09bbe8e36092a9cc05dd3fa67e3841e206e8c30ae0dd7032df4fff",
        "withdrawal_requests": [],
        "hash": "0x4ac339b063e52dac1b845d935788f379ebcdb0e33ecce077519f39929dbc8829"
    }
}
```

### Method `gw_get_block_committed_info`
* params:
    * `block_hash`: [`H256`](#type-h256)
* result: [`L2BlockCommittedInfo`](#type-l2blockcommittedinfo) `|` `null`

Get block layer1 committed info.

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_get_block_committed_info",
    "params": ["0xbf55ed82cf4b33a83df679b6cba8444a3527b64735d5b5c73f6163c24af525aa"]
}
```

Response

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "result": {
        "block_hash": "0x1f2a9c3aac8170d4ed82403298d4544955e3ce01dd8ee8e2ce591a1c67fe1b25",
        "number": "0xd2",
        "transaction_hash": "0x94ae05e36c0b6be0ee26a276dfc32f0cd3a0b1ab4da47812de369ef05562020d"
    }
}
```


### Method `gw_get_balance`
* params:
    * `registry_address`: [`SerializedRegistryAddress`](#type-serializedregistryaddress) - Serialized registry address
    * `sudt_id`: [`Uint32`](#type-uint32) - Simple UDT account ID
    * `block_number`(optional): [`Uint64`](#type-uint64) - block number, default is tip
* result: [`Uint256`](#type-uint256)

Get balance.

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_get_balance",
    "params": ["0x0200000014000000bb1d13450cfa630728d0390c99957c6948bf7d19", "0x1"]
}
```

Response

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "result": "0x9502f9000"
}
```

### Method `gw_get_storage_at`
* params:
    * `account_id`: [`Uint32`](#type-uint32) - Account ID
    * `key`: [`H256`](#type-h256) - Storage key
    * `block_number`(optional): [`Uint64`](#type-uint64) - block number, default is tip
* result: [`H256`](#type-h256)

Get storage at.

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_get_storage_at",
    "params": ["0x1", "0x0000000000000000000000000000000000000000000000000000000000000000"]
}
```

Response

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "result": "0x0000000000000000000000000000000000000000000000000000000000000000"
}
```

### Method `gw_get_account_id_by_script_hash`
* params:
    * `script_hash`: [`H256`](#type-h256) - Script Hash
* result: [`Uint32`](#type-uint32) `|` `null`

Get account ID by script hash.

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_get_account_id_by_script_hash",
    "params": ["0xdfb94d6794165b96668b4308607afc05790dc2110867d3370ceb8a412902e7b4"]
}
```

Response

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "result": "0x2"
}
```

### Method `gw_get_nonce`
* params:
    * `account_id`: [`H256`](#type-h256) - Account ID
    * `block_number`(optional): [`Uint64`](#type-uint64) - block number, default is tip
* result: [`Uint32`](#type-uint32) `|` `null`

Get account nonce.

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_get_nonce",
    "params": ["0x2"]
}
```

Response

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "result": "0x1"
}
```

### Method `gw_get_script`
* params:
    * `script_hash`: [`H256`](#type-h256) - Script Hash
* result: [`Script`](#type-script) `|` `null`


Get script by script hash.

#### Params

* Script Hash

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_get_script",
    "params": ["0xdfb94d6794165b96668b4308607afc05790dc2110867d3370ceb8a412902e7b4"]
}
```

Response

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "result": {
        "args": "0x828b8a63f97e539ddc79e42fa62dac858c7a9da222d61fc80f0d61b44b5af5d46daf63d8411d6e23552658e3cfb48416a6a2ca78",
        "code_hash": "0xf96d799a3c90ac8e153ddadd1747c6067d119a594f7f1c4b1fffe9db0f304335",
        "hash_type": "type"
    }
}
```

### Method `gw_get_script_hash`
* params:
    * `account_id`: [`Uint32`](#type-uint32) - Account ID
* result: [`H256`](#type-h256)

Get script hash.

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_get_script_hash",
    "params": ["0x2"]
}
```

Response

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "result": "0xdfb94d6794165b96668b4308607afc05790dc2110867d3370ceb8a412902e7b4"
}
```

### Method `gw_get_script_hash_by_registry_address`
* params:
    * `serialized_address`: [`SerializedRegistryAddress`](#type-serializedregistryaddress) - Serialized registry address
* result: [`H256`](#type-h256) `|` `null`

Get script hash by registry address.

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_get_script_hash_by_registry_address",
    "params": ["0x0200000014000000bb1d13450cfa630728d0390c99957c6948bf7d19"]
}
```

Response

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "result": "0xdfb94d6794165b96668b4308607afc05790dc2110867d3370ceb8a412902e7b4"
}
```

### Method `gw_get_registry_address_by_script_hash`
* params:
    * `script_hash`: [`H256`](#type-h256) - Script hash
    * `registry_id`: [`Uint32`](#type-uint32) - Registry ID (The builtin ID is 2 for Ethereum registry)
* result: [`RegistryAddress`](#type-registryaddress) `|` `null`

Get registry address by script hash.

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_get_registry_address_by_script_hash",
    "params": ["0x0003dfb94d6794165b96668b4308607afc05790dc211", "0x2"]
}
```

Response

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "result": "0x0200000014000000bb1d13450cfa630728d0390c99957c6948bf7d19"
}
```

### Method `gw_get_data`
* params:
    * `data_hash`: [`H256`](#type-h256) - Data Hash
    * `block_number`(optional): [`Uint64`](#type-uint64) - block number, default is tip
* result: [`JsonBytes`](#type-jsonbytes) `|` `null`

Get Data.

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_get_data",
    "params": ["0x0000000000000000000000000000000000000000000000000000000000000000"]
}
```

Response

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "result": null
}
```


### Method `gw_get_transaction`
* params:
    * `tx_hash`: [`H256`](#type-h256) - Transaction Hash
    * `verbose`(optional): `Uint8` - 0: Verbose; 1: Only Status. default is 0
* result: [`L2TransactionWithStatus`](#type-l2transactionwithstatus) `|` `null`

Get transaction.

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_get_transaction",
    "params": ["0x57c521ce4282fcf075862089d1bef4096723395ace63b4c0b8b9af5fa"]
}
```

Response

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "result": {
        "status": "pending",
        "transaction": {
            "hash": "0x57c521ce4282fcf075862089d1bef4096723395ace63b4c0b8b9af5faf924c55",
            "raw": {
                "args": "0xffffff504f4c590040420f0000000000000000000000000000000000000000000000000000000000000000000000000024000000fca3b5aa0000000000000000000000004ec86a4bd8b06d54d3e2ad96b20a374335e5b8f5",
                "from_id": "0x4",
                "nonce": "0x2f",
                "to_id": "0x18"
            },
            "signature": "0x30a37aabf68715f99ca88b21e49ca0f83ed329613e2e439c57cc2df2e65f836c3b1ed5b891cf39cae4ff6e0f0fc9660f96eec9b3ecf7a1df1f9cf0644c00efff01"
        }
    }
}
```

### Method `gw_get_transaction_receipt`
* params:
    * `tx_hash`: [`H256`](#type-h256) - Transaction Hash
* result: [`L2TransactionReceipt`](#type-l2transactionreceipt) `|` `null`


Get transaction receipt.

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_get_transaction_receipt",
    "params": ["0x57c521ce4282fcf075862089d1bef4096723395ace63b4c0b8b9af5fa"]
}
```

Response

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "result": {
        "exit_code": "0x0",
        "logs": [
            {
                "account_id": "0x1",
                "data": "0x14afc22cc46a66350eb9375e968b66bc544189e15dff6b6da3ce1f35bce453ae384bfc8b5532e6429700000000000000000000000000000000",
                "service_flag": "0x0"
            },
            {
                "account_id": "0x18",
                "data": "0x64570000000000006457000000000000000000000000000000000000000000000000000000000000",
                "service_flag": "0x2"
            },
            {
                "account_id": "0x1",
                "data": "0x14afc22cc46a66350eb9375e968b66bc544189e15d0cc94282bd0c6baed74078c0c7ab7943cbf71f7e00000000000000000000000000000000",
                "service_flag": "0x1"
            }
        ],
        "post_state": {
            "count": "0x1b",
            "merkle_root": "0x693321c3d1047557dc8d7082c33ec717df55546e30b6d9c1c98aadef31f653fa"
        },
        "read_data_hashes": [
            "0x04a263649046d6127a5c823deb75e1a6d52fc45ce7beef6de7ebbe6ee5ee2c56"
        ],
        "tx_witness_hash": "0xce2c35e321081fbe0c266048a920008033d2ac849c0427dd0db0e057e0c4471c"
    }
}
```

### Method `gw_get_withdrawal`
* params:
    * `withdrawal_hash`: [`H256`](#type-h256) - Withdrawal Hash
    * `verbose`(optional): `Uint8` - 0: Verbose; 1: Only Status. default is 0
* result: [`WithdrawalWithStatus`](#type-withdrawalwithstatus) `|` `null`


Get withdrawal info.

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_get_withdrawal",
    "params": ["0x3c4772eeef6d2c43b4ead9db7c049202d1f0b9e1bb075d08da1ab821e42a6859"]
}
```

Response

``` json
{
    "jsonrpc": "2.0",
    "id": 42,
    "result": {
        "withdrawal": {
            "request": {
                "raw": {
                    "nonce": "0x0",
                    "capacity": "0x9502f9000",
                    "amount": "0x0",
                    "sudt_script_hash": "0x0000000000000000000000000000000000000000000000000000000000000000",
                    "account_script_hash": "0x2a4504af3dccb71910d4fd70074de7a6aaea1f3140c97572155a2969e8a1aa16",
                    "registry_id": "0x2",
                    "owner_lock_hash": "0x651e4345ce3a3a7c4fcb1f78dc8fac799836da84ac8bf7d6e09f63b428875317",
                    "chain_id": "0x116e9",
                    "fee": "0x0"
                },
                "signature": "0x361815da3468b2cd03999252d8f0c16242fa5d619e37dd25259c146fd40c71c51ebd564eb7ee54ba83dbb78ba9ecfbbd6adc06aa426ada1ca96af66711d9a4f71c"
            },
            "owner_lock": {
                "code_hash": "0x79f90bb5e892d80dd213439eeab551120eb417678824f282b4ffb5f21bad2e1e",
                "hash_type": "type",
                "args": "0x019e18f89a2c541c259b40464fe9f1c8760722797200"
            }
        },
        "status": "committed"
    }
}
```

### Method `gw_execute_l2transaction`
* params:
    * `l2tx`: [`SerializedL2Transaction`](#type-serializedmoleculeschema) - Serialized L2 Transaction
* result: [`RunResult`](#type-runresult)


Execute layer2 transaction.

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_execute_l2transaction",
    "params": ["0x84010000100000006c010000800100005c01000014000000180000001c0000002000000002000000a30000001a00000038010000ffffff504f4c590020bcbe0000000000000000000000000000000000000000000000000000000000000000000000000004010000252dba420000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000000200000000000000000000000008be87ac9376c33c64583d0cd512227151fed5bfe000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000244d2301cc000000000000000000000000333c37400c7a519205554c2e9c3d4f2d750a42f800000000000000000000000000000000000000000000000000000000140000000c00000010000000000000000400000000000000"]
}
```

Response

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "result": {
        "logs": [
            {
                "account_id": "0x1",
                "data": "0x1468f5cea51fa6fcfdcc10f6cddcafa13bf67174368be87ac9376c33c64583d0cd512227151fed5bfe00000000000000000000000000000000",
                "service_flag": "0x0"
            },
            {
                "account_id": "0x1",
                "data": "0x148be87ac9376c33c64583d0cd512227151fed5bfe8be87ac9376c33c64583d0cd512227151fed5bfe00000000000000000000000000000000",
                "service_flag": "0x0"
            },
            {
                "account_id": "0xa3",
                "data": "0xb312000000000000b312000000000000000000000000000000000000000000000000000000000000",
                "service_flag": "0x2"
            },
            {
                "account_id": "0x1",
                "data": "0x1468f5cea51fa6fcfdcc10f6cddcafa13bf671743668f5cea51fa6fcfdcc10f6cddcafa13bf671743600000000000000000000000000000000",
                "service_flag": "0x1"
            }
        ],
        "return_data": "0x00000000000000000000000000000000000000000000000000000000000320200000000000000000000000000000000000000000000000000000000000000040000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000531edbb201"
    }
}

```

### Method `gw_execute_raw_l2transaction`
* params:
    * `raw_l2tx`: [`SerializedRawL2Transaction`](#type-serializedmoleculeschema) - Serialized Raw L2 Transaction
    * `block_number`(optional): [`Uint64`](#type-uint64) - block number, default is tip
* result: [`RunResult`](#type-runresult)


Execute layer2 transaction without signature.

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_execute_raw_l2transaction",
    "params": ["0x84010000100000006c010000800100005c01000014000000180000001c0000002000000002000000a30000001a00000038010000ffffff504f4c590020bcbe0000000000000000000000000000000000000000000000000000000000000000000000000004010000252dba420000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000000200000000000000000000000008be87ac9376c33c64583d0cd512227151fed5bfe000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000244d2301cc000000000000000000000000333c37400c7a519205554c2e9c3d4f2d750a42f800000000000000000000000000000000000000000000000000000000140000000c00000010000000000000000400000000000000"]
}
```

Response

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "result": {
        "logs": [
            {
                "account_id": "0x1",
                "data": "0x1468f5cea51fa6fcfdcc10f6cddcafa13bf67174368be87ac9376c33c64583d0cd512227151fed5bfe00000000000000000000000000000000",
                "service_flag": "0x0"
            },
            {
                "account_id": "0x1",
                "data": "0x148be87ac9376c33c64583d0cd512227151fed5bfe8be87ac9376c33c64583d0cd512227151fed5bfe00000000000000000000000000000000",
                "service_flag": "0x0"
            },
            {
                "account_id": "0xa3",
                "data": "0xb312000000000000b312000000000000000000000000000000000000000000000000000000000000",
                "service_flag": "0x2"
            },
            {
                "account_id": "0x1",
                "data": "0x1468f5cea51fa6fcfdcc10f6cddcafa13bf671743668f5cea51fa6fcfdcc10f6cddcafa13bf671743600000000000000000000000000000000",
                "service_flag": "0x1"
            }
        ],
        "return_data": "0x00000000000000000000000000000000000000000000000000000000000320200000000000000000000000000000000000000000000000000000000000000040000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000531edbb201"
    }
}
```

### Method `gw_compute_l2_sudt_script_hash`
* params:
    * `l1_sudt_script_hash`: [`H256`](#type-h256) - Layer1 Simple UDT type hash
* result: [`H256`](#type-h256)

Compute layer2 Simple UDT script hash

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_compute_l2_sudt_script_hash",
    "params": ["0x0000000000000000000000000000000000000000000000000000000000000000"]
}
```

Response

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "result": "0x99d75f9b654762fb822fb36dcb89de0cd385f0d1deff8f8d3430b7b93aca0597"
}
```

### Method `gw_get_fee_config`
* params: None
* result: [`FeeConfig`](#type-feeconfig)

Get fee config

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_get_fee_config",
    "params": []
}
```

Response

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "result": {
        "meta_cycles_limit": "0x4e20",
        "sudt_cycles_limit": "0x4e20",
        "withdraw_cycles_limit": "0x4e20"
    }
}
```

### Method `gw_submit_l2transaction`
* params:
    * `l2tx`: [`SerializedL2Transaction`](#type-serializdmoleculeschema) - L2 transaction
* result: [`H256`](#type-h256)

Submit layer2 transaction. This RPC may has rate limit.

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_submit_l2transaction",
    "params": ["0xb5010000100000009d010000b10100008d0100000c000000480100003c01000014000000180000001c000000200000003100000084000000d602000018010000ffffff504f4c590020bcbe00000000000000000000000000000000000000000000e1f505000000000000000000000000e40000007ff36ab5000000000000000000000000000000000000000000000000115f08c6acba85c20000000000000000000000000000000000000000000000000000000000000080000000000000000000000000333c37400c7a519205554c2e9c3d4f2d750a42f80000000000000000000000000000000000000000000000000000000061b967dc00000000000000000000000000000000000000000000000000000000000000020000000000000000000000007417e92923952a3d65bffab3f34d2bd77497c890000000000000000000000000c5e133e6b01b2c335055576c51a53647b1b9b6244100000097616f7d50457b01bdf55e48d967f3a458274affedb4b071e4f5c6ea34a8d2283c71683bd51ce8d678dd36be2c13cc6b48753d923ddc10d6c4c53d3947395ddf00140000000c00000010000000000000000400000000000000"]
}
```

Response

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "result": "0xf3ccf2bd7b22885dbdcd837d4a0aad30c70a84319016644f0d94e2f4135f1ade"
}
```

### Method `gw_submit_withdrawal_request`
* params:
    * `withdrawal_request`: [`SerializedWithdrawRequest`](#type-serializedmoleculeschema) - L2 withdrawal
* result: [`H256`](#type-h256)

Submit layer2 withdrawal request

#### Examples
   
Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_submit_withdrawal_request",
    "params": ["0x190100000c000000d4000000d5020000003bc09109000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000333c37400c7a519205554c2e9c3d4f2d750a42f81661dfc4da4ce3e20a6bd23c0000000000000000000000000000000000000000000000009cb93d3362f5d511eb5baa98c9d5da8ada50161798c8800dde4b15b6531595f900000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000410000000193740968815ce5a89a1c3a781ce44e0e16bf031d79c66056f56f3621dba5b0103d51bdf471f038feadf9e55fe00d09dd64aa02642b7327ab680d7d9f04f89e01"]
}
```

Response

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "result": "0xb57c6da2f803413b5781f8c6508320a0ada61a2992bb59ab38f16da2d02099c1"
}
```

### Method `gw_get_last_submitted_info`
* params: None
* result: [`LastL2BlockCommittedInfo`](#type-lastl2blockcommittedinfo)

Get node last submitted info.

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_get_last_submitted_info",
    "params": []
}
```

Response

``` json
 {
    "id": 42,
    "jsonrpc": "2.0",
    "result": {
        "transaction_hash": "0x1536b5af1e42707e0278cf16dd086ec630485883ce3d1c1388f9eb4d8169b119"
    }
}
```

### Method `gw_get_mem_pool_state_root`
* params: None
* result: [`H256`](#type-h256)

Get mem-pool state root.

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_get_mem_pool_state_root",
    "params": []
}
```

Response

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "result": "0xf3349effe912609ab277e227925995070ea8f3e452854852ed7386206371f07d"
}
```


### Method `gw_get_mem_pool_state_ready`
* params: None
* result: `true` `|` `false`

Get mem-pool state root.

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_get_mem_pool_state_ready",
    "params": []
}
```

Response

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "result": true
}
```



## RPC Types

### Type `Uint32`

The 32-bit unsigned integer type encoded as the 0x-prefixed hex string in JSON.

#### Examples

|  JSON | Decimal Value |
| --- |--- |
|  “0x0” | 0 |
|  “0x10” | 16 |
|  “10” | Invalid, 0x is required |
|  “0x01” | Invalid, redundant leading 0 |

### Type `Uint64`

The 64-bit unsigned integer type encoded as the 0x-prefixed hex string in JSON.

#### Examples

|  JSON | Decimal Value |
| --- |--- |
|  “0x0” | 0 |
|  “0x10” | 16 |
|  “10” | Invalid, 0x is required |
|  “0x01” | Invalid, redundant leading 0 |

### Type `Uint128`

The 128-bit unsigned integer type encoded as the 0x-prefixed hex string in JSON.

#### Examples

|  JSON | Decimal Value |
| --- |--- |
|  “0x0” | 0 |
|  “0x10” | 16 |
|  “10” | Invalid, 0x is required |
|  “0x01” | Invalid, redundant leading 0 |


### Type `Uint256`

The 256-bit unsigned integer type encoded as the 0x-prefixed hex string in JSON.

#### Examples

|  JSON | Decimal Value |
| --- |--- |
|  “0x0” | 0 |
|  “0x10” | 16 |
|  “10” | Invalid, 0x is required |
|  “0x01” | Invalid, redundant leading 0 |

### Type `H256`

The 32-byte fixed-length binary data.

The name comes from the number of bits in the data.

In JSONRPC, it is encoded as a 0x-prefixed hex string.

#### Fields

`H256` is a JSON object with the following fields.

*   `0`: https://doc.rust-lang.org/1.56.1/std/primitive.array.html - Converts `Self` to a byte slice.

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


### Type `Backend`

#### Fields

`Backend` is a JSON object with the following fields.

*   `validator_code_hash`: [`H256`](#type-h256) - Validator script's code hash

*   `generator_code_hash`: [`H256`](#type-h256) - Generator script's code hash

*   `validator_script_type_hash`: [`H256`](#type-h256) - Validate script's hash (script.hash_type = type)


### Type `NodeInfo`


#### Fields

`NodeInfo` is a JSON object with the following fields.

*   `mode`: `fullnode` `|` `test` `|` `readonly` - Node mode

*   `backends`: [`Backend[]`](#type-backend) - Backend infos

*   `version`: `string` - Version of current godwoken node

*   `eoa_scripts`: [`EoaScript[]`](#type-eoascript)

*   `gw_scripts`: [`GwScript[]`](#type-gwscript)

*   `rollup_cell`: [`RollupCell`](#type-rollupcell)

*   `rollup_config`: [`NodeRollupConfig`](#type-noderollupconfig)


### Type `EoaScript`

#### Fields

`EoaScript` is a JSON object with the following fields.

*   `type_hash`: [`H256`](#type-h256)

*   `script`: [`Script`](#type-script)

*   `eoa_type`: `unknown` `|` `eth`

### Type `GwScript`

#### Fields

`GwScript` is a JSON object with the following fields.

*   `type_hash`: [`H256`](#type-h256)

*   `script`: [`Script`](#type-script)

*   `script_type`: `unknown` `|` `deposit` `|` `withdraw` `|` `state_validator` `|` `stake_lock` `|` `custodian_lock` `|` `challenge_lock` `|` `l1_sudt` `|` `l2_sudt` `|` `omni_lock`

### Type `RollupCell`

#### Fields

`RollupCell` is a JSON object with the following fields.

*   `type_hash`: [`H256`](#type-h256)

*   `type_script`: [`Script`](#type-script)

### Type `NodeRollupConfig`

#### Fields

`NodeRollupConfig` is a JSON object with the following fields.

*   `required_staking_capacity`: [`Uint64`](#type-uint64)

*   `challenge_maturity_blocks`: [`Uint64`](#type-uint64)

*   `finality_blocks`: [`Uint64`](#type-uint64)

*   `reward_burn_rate`: [`Uint32`](#type-uint32)

*   `chain_id`: [`Uint64`](#type-uint64)


### Type `L2BlockWithStatus`

#### Fields

`L2BlockWithStatus` is a JSON object with the following fields.

*   `block`: [`L2Block`](#type-l2block) - L2 block

*   `status`: `finalized` `|` `unfinalized` `|` `reverted` - L2 block status
    * `finalized`: Block already finalized

    * `unfinalized`: Block not already finalized

    * `reverted`: Block has already reverted


### Type `L2Block`

### Fields

`L2Block` is a JSON object with the following fields.

*   `block_proof`: [`JsonBytes`](#type-jsonbytes)

*   `hash`: [`H256`](#type-h256) - Block hash

*   `kv_state`: [`KVPair[]`](#type-kvpair)

*   `kv_state_proof`: [`JsonBytes`](#type-jsonbytes)

*   `raw`: [`RawL2Block`](#type-rawl2block)

*   `transactions`: [`L2Transaction[]`](#type-l2transaction)

*   `withdrawal_requests`: [`WithdrawalRequest[]`](#type-withdrawalrequest)

### Type `KVPair`


#### Fields

`KVPair` is a JSON object with the following fields.

*   `k`: [`H256`](#type-h256)

*   `v`: [`H256`](#type-h256)


### Type `RawL2Block`


#### Fields

`RawL2Block` is a JSON object with the following fields.

*   `block_producer`: [`SerializedRegistryAddress`](#type-serializedregistryaddress) - Block producer's registry address

*   `parent_block_hash`: [`H256`](#type-h256) - Prev block hash

*   `post_account`: [`AccountInfo`](#type-accountinfo)

*   `prev_account`: [`AccountInfo`](#type-accountinfo)

*   `stake_cell_owner_lock_hash`: [`h256`](#type-h256)

*   `state_checkpoint_list`: [`h256[]`](#type-256)

*   `submit_transactions`: [`SubmitTransaction[]`](#type-submittransaction)

*   `submit_withdrawals`: [`SubmitWithdrawal[]`](#type-submitwithdrawal)

*   `timestamp`: [`Uint64`](#type-uint64)


### Type `AccountInfo`


#### Fields

`AccountInfo` is a JSON object with the following fields.

*   `count`: [`Uint32`](#type-uint32)

*   `merkle_root`: [`H256`](#type-h256)

### Type `AccountMerkleState`


#### Fields

`AccountMerkleState` is a JSON object with the following fields.

*   `prev_state_checkpoint`: [`H256`](#type-h256)

*   `tx_count`: [`Uint32`](#type-uint32)

*   `tx_witness_root`: [`H256`](#type-h256)


### Type `SubmitWithdrawal`


#### Fields

`AccountInfo` is a JSON object with the following fields.

*   `withdrawal_count`: [`Uint32`](#type-uint32)

*   `withdrawal_witness_root`: [`H256`](#type-h256)


### Type `L2TransactionWithStatus`

#### Fields

`L2TransactionWithStatus` is a JSON object with the following fields.

*   `transaction`: [`L2Transaction`](#type-l2transaction)

*   `status`: `pending` `|` `committed`



### Type `L2Transaction`

#### Fields

`L2Transaction` is a JSON object with the following fields.

*   `raw`: [`RawL2Transaction`](#type-rawl2transaction) `|` `null`

*   `signature`: [`JsonBytes`](#type-jsonbytes)

*   `hash`: [`H256`](#type-h256) - Transaction hash


### Type `RawL2Transaction`

#### Fields

`RawL2Transaction` is a JSON object with the following fields.

*   `chain_id`: [`Uint64`](#type-uint64)

*   `from_id`: [`Uint32`](#type-uint32)

*   `to_id`: [`Uint32`](#type-uint32)

*   `nonce`: [`Uint32`](#type-uint32)

*   `args`: [`JsonBytes`](#type-jsonbytes)


### Type `L2TransactionReceipt`

#### Fields

`L2TransactionReceipt` is a JSON object with the following fields.

*   `tx_witness_hash`: [`H256`](#type-256)

*   `post_state`: [`AccountMerkleState`](#type-accountmerklestate)

*   `read_data_hashes`: [`H256[]`](#type-h256)

*   `logs`: [`LogItem[]`](#type-logitem)


### Type `LogItem`

#### Fields

`LogItem` is a JSON object with the following fields.

*   `account_id`: [`Uint32`](#type-uint32)

*   `service_flag`: [`Uint32`](#type-uint32)

*   `data`: [`JsonBytes`](#type-jsonbytes)

### Type `RunResult`

#### Fields

`RunResult` is a JSON object with the following fields.

*   `return_data`: [`JsonBytes`](#type-jsonbytes)

*   `logs`: [`LogItem[]`](#type-logitem)

### Type `FeeConfig`

#### Fields

`FeeConfig` is a JSON object with the following fields.

*   `meta_cycles_limit`: [`Uint64`](#type-uint64)

*   `sudt_cycles_limit`: [`Uint64`](#type-uint64)

*   `withdraw_cycles_limit`: [`Uint64`](#type-uint64)

### Type `WithdrawalWithStatus`

#### Fields

`WithdrawalWithStatus` is a JSON object with the following fields.

*   `withdrawal`: [`WithdrawalRequestExtra`](#type-withdrawalrequestextra) `|` `null`

*   `status`: `pending` `|` `committed`


### Type `WithdrawalRequestExtra`

#### Fields

`WithdrawalRequestExtra` is a JSON object with the following fields.

*   `request`: [`WithdrawalRequest`](#type-withdrawalrequest)

*   `owner_lock`: [`Script`](#type-script)


### Type `WithdrawalRequest`

#### Fields

`WithdrawalRequest` is a JSON object with the following fields.

*   `raw`: [`RawWithdrawalRequest`](#type-rawwithdrawalrequest)

*   `signature`: [`JsonBytes`](#type-jsonbytes)


### Type `RawWithdrawalRequest`

#### Fields

`RawWithdrawalRequest` is a JSON object with the following fields.

*   `nonce`: [`Uint32`](#type-uint32)

*   `capacity`: [`Uint64`](#type-uint64)

*   `amount`: [`Uint128`](#type-uint128)

*   `sudt_script_hash`: [`H256`](#type-h256)

*   `account_script_hash`: [`H256`](#type-h256)

*   `registry_id`: [`Uint32`](#type-uint32)

*   `owner_lock_hash`: [`H256`](#type-h256) - layer1 lock to withdraw after challenge period

*   `chain_id`: [`Uint64`](#type-uint64)

*   `fee`: [`Uint128`](#type-uint128)


### Type `L2BlockCommittedInfo`

#### Fields

`L2BlockCommittedInfo` is a JSON object with the following fields.

*   `number`: [`Uint64`](#type-uint64)

*   `block_hash`: [`H256`](#type-h256)

*   `transaction_hash`: [`H256`](#type-h256)


### Type `LastL2BlockCommittedInfo`

#### Fields

`LastL2BlockCommittedInfo` is a JSON object with the following fields.

*   `transaction_hash`: [`H256`](#type-h256)


### Type `RegistryAddress`

#### Fields

`RegistryAddress` is a JSON object with the following fields.

*   `registry_id`: [`Uint32`](#type-uint32)

*   `address`: [`JsonBytes`](#type-jsonbytes)

### Type `SerializedRegistryAddress`

It's a 0x-prefix hex string in JSON.

#### Examples

```
registry_address = 0x0200000014000000bb1d13450cfa630728d0390c99957c6948bf7d19
```

#### Fields

```
registry_address = 0x | registry_account id | address_size | address
```

*   `registry_account_id`:
    * 4-byte, Uint32 in little endian format.
    * In example, it's `02000000`, means id is `2`.
    * The builtin ID is 2 for Ethereum registry.

*   `address_size`:
    * Byte length of address, 4-byte, Uint32 in little endian format.
    * In example, byte length is `14000000`, means address length is 20-byte.

*   `address`: [`JsonBytes`](#type-jsonbytes)
    * Addess such as Eth Address.
    * In example, address is `bb1d13450cfa630728d0390c99957c6948bf7d19`


### Type `SerializedMoleculeSchema`

It's a 0x-prefix hex string in JSON. Serialized by [Molecule](https://github.com/nervosnetwork/molecule).

See schema files for more info.

*   `SerializedL2Transaction`: [schema](../crates/types/schemas/godwoken.mol#L78-L81)
*   `SerializedRawL2Transaction`: [schema](../crates/types/schemas/godwoken.mol#L69-L76)
*   `SerializedWithdrawRequest`: [schema](../crates/types/schemas/godwoken.mol#L157-L160)

### Type `Script`

More info [CKB RPC](https://github.com/nervosnetwork/ckb/blob/develop/rpc/README.md#type-script)

### Type `ScriptHashType`

More info [CKB RPC](https://github.com/nervosnetwork/ckb/blob/develop/rpc/README.md#type-scripthashtype)
