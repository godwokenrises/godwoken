# RPC

## Table of Contents

* [RPC Methods](#rpc-methods)
    * [Method `gw_ping`](#method-gw_ping)
    * [Method `gw_get_node_info`](#method-gw_get_node_info)
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
    * [Method `gw_submit_l2transaction`](#method-gw_submit_l2transaction)
    * [Method `gw_submit_withdrawal_request`](#method-gw_submit_withdrawal_request)
    * [Method `gw_get_last_submitted_info`](#method-gw_get_last_submitted_info)
    * [Method `gw_get_mem_pool_state_root`](#method-gw_get_mem_pool_state_root)
* [RPC Types](#rpc-types)
    * [Type `Uint32`](#type-uint32)
    * [Type `Uint64`](#type-uint64)
    * [Type `Uint128`](#type-uint128)
    * [Type `Uint256`](#type-uint256)
    * [Type `H256`](#type-h256)
    * [Type `JsonBytes`](#type-jsonbytes)
    * [Type `Backend`](#type-backend)
    * [Type `NodeInfo`](#type-nodeinfo)
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
    * [Type `LastL2BlockCommittedInfo`](#type-lastl2blockcommittedinfo)
    * [Type `SerializedRegistryAddress`](#type-serializedregistryaddress)
    * [Type `SerializedL2Transaction`](#type-serializedmoleculeschema)
    * [Type `SerializedRawL2Transaction`](#type-serializedmoleculeschema)
    * [Type `SerializedWithdrawalRequest`](#type-serializedmoleculeschema)
    * [Type `Script`](#type-script)
    * [Type `ScriptHashType`](#type-scripthashtype)
    

## Methods

### Method `gw_ping`

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
    "id": 42,
    "jsonrpc": "2.0",
    "result": {
        "backends": [
            {
                "generator_code_hash": "0x8ce08586eca43c72c720737af48ec515b1caec8d369dbed71a627f5bcef63eb4",
                "validator_code_hash": "0x5f7054ae0a66a6a7fc9e45d5339035a620fb42677659cd3d6c90221aa8db47f2",
                "validator_script_type_hash": "0x6677005599a98f86f003946eba01a21b54ed1f13a09f36b5e8bbcf7586b96b41"
            },
            {
                "generator_code_hash": "0x6c14c12165d27ec773438c73143adb051d15f0357084c39e54f84a1bfa79194a",
                "validator_code_hash": "0x6523b2f1ea42620e40c3be7a64ecb195dfda08ae6106f475c2a38f9dafd27e0b",
                "validator_script_type_hash": "0x61dbbe7a228d4340a869c81748fed4c3dc5d597bb0fb0c0fa3d17a8230b51440"
            },
            {
                "generator_code_hash": "0xe0c7e13381bae7973d71b1e9683044714ee8ec28b27e913bad0b3c211fe5877c",
                "validator_code_hash": "0x8fbb70300b2873f98df2d1a4f74b40a64b7f15b90fb9b835dde8f828585a9835",
                "validator_script_type_hash": "0xa78176967a0164dc35b9c5b8c83635f65c72a3715db0b589f278507a3937592b"
            }
        ],
        "version": "0.7.0"
    }
}
```

### Method `gw_get_tip_block_hash`

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

Get block.

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_get_block",
    "params": ["0xbf55ed82cf4b33a83df679b6cba8444a3527b64735d5b5c73f6163c24af525aa"]
}
```

Response

``` json
{
    "jsonrpc": "2.0",
    "result": {
        "block": {
            "block_proof": "0x4c4f0150d34fd947b81c2c60a7777d87c228e6565a30c653f8bcdda9f6b9c374d7fa96884f015023aefeaf5cedf8a3d5826d69d49e0f814ae3bd201bcbbe40b4b4e18a85ebb6354f015074b9c0407ea1d814c9ce19e65dd948cdb767f4a3189c84a39e82aa2be419e4454ffa",
            "hash": "0xbf55ed82cf4b33a83df679b6cba8444a3527b64735d5b5c73f6163c24af525aa",
            "kv_state": [],
            "kv_state_proof": "0x",
            "raw": {
                "block_producer_id": "0x0",                                                                                             "number": "0x2a",
                "parent_block_hash": "0x082e50475067310505e1e2dccc8d88158722858cc43a15e417a4c4210c56ab80",
                "post_account": {
                    "count": "0x18",
                    "merkle_root": "0xf3349effe912609ab277e227925995070ea8f3e452854852ed7386206371f07d"
                },
                "prev_account": {
                    "count": "0x18",
                    "merkle_root": "0xf3349effe912609ab277e227925995070ea8f3e452854852ed7386206371f07d"
                },
                "stake_cell_owner_lock_hash": "0x3bab60cef4af81a87b0386f29bbf1dd0f6fe71c9fe1d84ca37096a6284d3bdaf",
                "state_checkpoint_list": [],
                "submit_transactions": {
                    "prev_state_checkpoint": "0x82e15c5f8a97bbce6dc56e6fbf352e7babd5de8b8d9af4b64a76c2d933e5818d",
                    "tx_count": "0x0",
                    "tx_witness_root": "0x0000000000000000000000000000000000000000000000000000000000000000"
                },
                "submit_withdrawals": {
                    "withdrawal_count": "0x0",
                    "withdrawal_witness_root": "0x0000000000000000000000000000000000000000000000000000000000000000"
                },
                "timestamp": "0x17da80a7770"
            },
            "transactions": [],
            "withdrawal_requests": []
        },
        "status": "finalized"
    }
}
```

### Method `gw_get_block_by_number`

Get block by number.

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_get_block_by_number",
    "params": ["0x2a"]
}
```

Response

``` json
{
    "jsonrpc": "2.0",
    "result": {
        "block_proof": "0x4c4f0150d34fd947b81c2c60a7777d87c228e6565a30c653f8bcdda9f6b9c374d7fa96884f015023aefeaf5cedf8a3d5826d69d49e0f814ae3bd201bcbbe40b4b4e18a85ebb6354f015074b9c0407ea1d814c9ce19e65dd948cdb767f4a3189c84a39e82aa2be419e4454ffa",
        "hash": "0xbf55ed82cf4b33a83df679b6cba8444a3527b64735d5b5c73f6163c24af525aa",
        "kv_state": [],
        "kv_state_proof": "0x",
        "raw": {
            "block_producer_id": "0x0",                                                                                             "number": "0x2a",
            "parent_block_hash": "0x082e50475067310505e1e2dccc8d88158722858cc43a15e417a4c4210c56ab80",
            "post_account": {
                "count": "0x18",
                "merkle_root": "0xf3349effe912609ab277e227925995070ea8f3e452854852ed7386206371f07d"
            },
            "prev_account": {
                "count": "0x18",
                "merkle_root": "0xf3349effe912609ab277e227925995070ea8f3e452854852ed7386206371f07d"
            },
            "stake_cell_owner_lock_hash": "0x3bab60cef4af81a87b0386f29bbf1dd0f6fe71c9fe1d84ca37096a6284d3bdaf",
            "state_checkpoint_list": [],
            "submit_transactions": {
                "prev_state_checkpoint": "0x82e15c5f8a97bbce6dc56e6fbf352e7babd5de8b8d9af4b64a76c2d933e5818d",
                "tx_count": "0x0",
                "tx_witness_root": "0x0000000000000000000000000000000000000000000000000000000000000000"
            },
            "submit_withdrawals": {
                "withdrawal_count": "0x0",
                "withdrawal_witness_root": "0x0000000000000000000000000000000000000000000000000000000000000000"
            },
            "timestamp": "0x17da80a7770"
        },
        "transactions": [],
        "withdrawal_requests": []
    }
}
```

### Method `gw_get_block_committed_info`

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

Get balance.

#### Params

* Serialized registry address
* Simple UDT account ID
* (Optional) block number, default is tip
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

Get storage at.

#### Params

* Account ID
* Storage key
* (Optional) block number, default is tip

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

Get account ID by script hash.

#### Params

* Script Hash

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

Get account nonce.

#### Params

* Account ID
* (Optional) block number, default is tip

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

Get script hash.

#### Params

* Account ID

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

Get script hash by registry address.

#### Params

* Serialized registry address

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

Get registry address by script hash.

#### Params

* Script hash
* Registry ID (The builtin ID is 2 for Ethereum registry)

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

Get Data.

#### Params

* Data Hash

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

Get transaction.

#### Params

* Transaction Hash
* (Optional) 0: Verbose; 1: Only Status. default is 0

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

Get transaction receipt.

#### Params

* Transaction Hash

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

Get withdrawal info.

#### Params

* Withdrawal Hash
* (Optional) 0: Verbose; 1: Only Status. default is 0

#### Examples

Request

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "method": "gw_get_withdrawal",
    "params": ["0xb57c6da2f803413b5781f8c6508320a0ada61a2992bb59ab38f16da2d02099c1"]
}
```

Response

``` json
{
    "id": 42,
    "jsonrpc": "2.0",
    "result": {
        "status": "committed",
        "withdrawal": {
            "raw": {
                "account_script_hash": "0x333c37400c7a519205554c2e9c3d4f2d750a42f81661dfc4da4ce3e20a6bd23c",
                "amount": "0x0",
                "capacity": "0x991c03b00",
                "fee": {
                    "amount": "0x0",
                    "sudt_id": "0x1"
                },
                "nonce": "0x2d5",
                "owner_lock_hash": "0x9cb93d3362f5d511eb5baa98c9d5da8ada50161798c8800dde4b15b6531595f9",
                "payment_lock_hash": "0x0000000000000000000000000000000000000000000000000000000000000000",
                "sell_amount": "0x0",
                "sell_capacity": "0x0",
                "sudt_script_hash": "0x0000000000000000000000000000000000000000000000000000000000000000"
            },
            "signature": "0x0193740968815ce5a89a1c3a781ce44e0e16bf031d79c66056f56f3621dba5b0103d51bdf471f038feadf9e55fe00d09dd64aa02642b7327ab680d7d9f04f89e01"
        }
    }
}
```

### Method `gw_execute_l2transaction`

Execute layer2 transaction.

#### Params

* L2 Transaction

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

Execute layer2 transaction without signature.

#### Params

* Raw L2 Transaction

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

Compute layer2 Simple UDT script hash

#### Params

* Layer1 Simple UDT type hash

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
        "sudt_fee_rate_weight": [],
        "withdraw_cycles_limit": "0x4e20"
    }
}
```

### Method `gw_submit_l2transaction`

Submit layer2 transaction. This RPC may has rate limit.

#### Params

* L2 transaction

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

Submit layer2 withdrawal request

#### Params

* L2 withdrawal

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

*   `backends`: [`Backend[]`](#type-backend) - Backend infos

*   `version`: `string` - Version of current godwoken node


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
