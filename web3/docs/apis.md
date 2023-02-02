# APIs

## Ethereum Compatible Web3 RPC Modules

### net

- net_version
- net_peerCount
- net_listening

### web3

- web3_sha3
- web3_clientVersion

### eth

- eth_chainId
- eth_protocolVersion
- eth_syncing
- eth_coinbase
- eth_mining
- eth_hashrate
- eth_gasPrice
- eth_blockNumber
- eth_getBalance
- eth_getStorageAt
- eth_getTransactionCount
- eth_getCode
- eth_call
- eth_estimateGas
- eth_getBlockByHash
- eth_getBlockByNumber
- eth_getBlockTransactionCountByHash
- eth_getBlockTransactionCountByNumber
- eth_getUncleByBlockHashAndIndex
- eth_getUncleByBlockNumberAndIndex
- eth_getUncleCountByBlockHash
- eth_getCompilers
- eth_getTransactionByHash
- eth_getTransactionByBlockHashAndIndex
- eth_getTransactionByBlockNumberAndIndex
- eth_getTransactionReceipt
- eth_newFilter
- eth_newBlockFilter
- eth_newPendingTransactionFilter
- eth_uninstallFilter
- eth_getFilterLogs
- eth_getFilterChanges
- eth_getLogs
- eth_subscribe (only for WebSocket)
- eth_unsubscribe (only for WebSocket)

### Usage

You can find most usage guidelines from Ethereum RPC docs like <https://eth.wiki/json-rpc/API>

### Unsupported Methods

- eth_accounts (only supported by wallet client)
- eth_sign (only supported by wallet client)
- eth_signTransaction (only supported by wallet client)
- eth_sendTransaction (only supported by wallet client)

## Additional Modules

### gw (Godwoken RPCs)

#### Methods

- gw_ping
- gw_get_tip_block_hash
- gw_get_block_hash
- gw_get_block
- gw_get_block_by_number
- gw_get_balance
- gw_get_storage_at
- gw_get_account_id_by_script_hash
- gw_get_nonce
- gw_get_script
- gw_get_script_hash
- gw_get_data
- gw_get_transaction_receipt
- gw_get_transaction
- gw_execute_l2transaction
- gw_execute_raw_l2transaction
- gw_submit_l2transaction
- gw_submit_withdrawal_request
- gw_get_registry_address_by_script_hash
- gw_get_script_hash_by_registry_address
- gw_get_fee_config
- gw_get_withdrawal
- gw_get_last_submitted_info
- gw_get_node_info
- gw_is_request_in_queue
- gw_get_pending_tx_hashes
- gw_debug_replay_transaction (should enable `Debug` RPC module in Godwoken)

#### Usage

Get details at [Godwoken Docs](https://github.com/godwokenrises/godwoken/blob/develop/docs/RPC.md)

### poly (Polyjuice RPCs)

#### Methods

- poly_getCreatorId
- poly_getDefaultFromId
- poly_getContractValidatorTypeHash
- poly_getRollupTypeHash
- poly_getEthAccountLockHash
- poly_version
- poly_getEthTxHashByGwTxHash
- poly_getGwTxHashByEthTxHash
- poly_getHealthStatus

#### Usage

Get details at [Poly APIs doc](poly-apis.md)

### debug (Debug RPCs)

#### Methods
- debug_replayTransaction (should enable `Debug` RPC module in Godwoken)
