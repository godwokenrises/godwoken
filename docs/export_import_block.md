# export/import block

Godwoken support export/import layer2 blocks from/to existing node database.

## Export block

To export layer2 block, using `godwoken export-block` subcommand. It will open database in readonly mode.
You don't need to exit running godwoken process to export block.

### example

```shell
godwoken export-block -c config.toml --output-path ./blocks_testnet_v1 --from-block 0 --to-block 100000 --show-progress
```

A binary file `blocks_testnet_v1_702359ea7f073558921eb50d8c1c77e92f760c8f8656bde4995f26b8963e2dd8_0_100000` will be generated.

NOTE: `702359ea7f073558921eb50d8c1c77e92f760c8f8656bde4995f26b8963e2dd8` is testnet_v1 rollup type hash.

## Import block

To import layer2 block, using `godwoken import-block` subcommand. You must exit running godwoken process to execute
this subcommand.

It will insert blocks into database `store.path` configurated in `config.toml`.

NOTE: a valid `ckb_url` in `config.toml` is required, because it needs to fetch secp data from ckb genesis block to open database.

### example

```shell
godwoken import-block -c config.toml --source-path ./blocks_testnet_v1_702359ea7f073558921eb50d8c1c77e92f760c8f8656bde4995f26b8963e2dd8_0_100000 --to-block 50000 --show-progress
```
