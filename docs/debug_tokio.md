# Debug Tokio

## tokio-metrics

**Runtime Monitor** is unstable. You need build with RUSTFLAGS as below:

```sh
RUSTFLAGS="--cfg tokio_unstable" cargo build
```

## tokio-console

**tokio-console** also uses unstable rust features.

- build with: RUSTFLAGS="--cfg tokio_unstable" cargo build
- install tokio-console: cargo install tokio-console
- export env varible before start godwoken: export TOKIO_CONSOLE_BIND = "0.0.0.0:6669"
- start tokio-console in your console just with(attatch to 6669 in default config): tokio-console