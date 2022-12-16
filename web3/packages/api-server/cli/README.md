# CLI

### Fix wrong transaction.eth_tx_hash

This problem fixed in `v1.5.1-rc1`, so data indexed by version `>= v1.5.1-rc1` is ok.

Run `list-wrong-eth-tx-hashes` to see how many wrong data in database.

`eth_tx_hash` was wrong before when R or S with leading zeros in database.

Run once after `web3-indexer` stopped is enough.

```
// List first 20 txs, database-url can also read from env
yarn run cli list-wrong-eth-tx-hashes -d <database url>
yarn run cli list-wrong-eth-tx-hashes --help // for more info

// Fix wrong data
// database-url can also read from env, and chain-id can also read from RPC, using `yarn run cli fix-eth-tx-hash --help` for more infomation.
yarn run cli fix-eth-tx-hash -d <database url> -c <chain id>
yarn run cli fix-eth-tx-hash --help // for more info
```

### Fix wrong log.transaction_index

Run `wrong-log-transaction-index-count` to see how many wrong data in database.

`log.transaction_index` always be zero when indexed after version `v1.6.0-rc1`.

Run once after `web3-indexer` updated is enough.

```
// Get count, database-url can also read from env
yarn run cli wrong-log-transaction-index-count -d <database url>
yarn run cli wrong-log-transaction-index-count --help // for more info

// Fix wrong data
// database-url can also read from env
yarn run cli fix-log-transaction-index -d <database url>
yarn run cli fix-log-transaction-index --help // for more info
```
