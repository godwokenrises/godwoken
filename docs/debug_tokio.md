# Debug Tokio

## tokio-metrics

**Runtime Monitor** is unstable. You can build with RUSTFLAGS as shown below:

```sh
RUSTFLAGS="--cfg tokio_unstable" cargo build
```

## tokio-console

**tokio-console** also uses unstable tokio features like the following:

- build with: ``` RUSTFLAGS="--cfg tokio_unstable" cargo build```
- install tokio-console: ``` cargo install tokio-console```
- export env varible before starting godwoken: ``` export TOKIO_CONSOLE_BIND = "0.0.0.0:6669"```
- start tokio-console in your console with(attatch to 6669 in default config):  ``` tokio-console```
