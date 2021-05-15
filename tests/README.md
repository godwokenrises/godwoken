This directory contains integration tests that test godwoken binary.

## Running tests locally
Before tests can be run locally, godwoken binary must be built.

The following command assumes that godwoken binary is built as `../target/debug/godwoken` and starting node on port 8119:

```bash
cargo run
```

Run specified specs:

```bash
cargo run -- --bin ../target/debug/godwoken spec1 spec2
```

See all available options:

```bash
cargo run -- --help
```
