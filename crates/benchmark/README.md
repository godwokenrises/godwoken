# Godwoken benchmark

## build

```shell
cargo install --path crates/benchmark
```

## run

### 1. generate a config file

```shell
gw-benchmark generate
```

Then, we will get a toml file looks like below:

```toml
interval = 1000
batch = 10
timeout = 120
account_path = "./accounts"
gw_rpc_url = "http://localhost:8119"
polyman_url = "http://localhost:6102"
scripts_deploy_path = "./scripts_deploy_results.json"
rollup_type_hash = "0x"
```

### 2. run benchmark with a config

```shell
gw-benchmark run -p `your-config.toml`
```

## Stats

Checkout the stats from info log.
