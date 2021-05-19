This directory contains integration tests that test godwoken binary.

## Running tests locally
Before tests can be run locally, a godwoken dev chain should be runing.
Please update your godwoken configs into `tests/configs`, including `godwoken-config.toml`, `scripts-deploy-result.json` and `lumos-config.json`.

```bash
./init.sh
source <example.env> # use your own env file containing RPC URLs and private keys etc.
cargo run
```


---
### TODO:
- [ ] design ./src/node.rs 

Run specified specs:

```bash
cargo run -- --bin ../target/debug/godwoken spec1 spec2
```

See all available options:

```bash
cargo run -- --help
```
