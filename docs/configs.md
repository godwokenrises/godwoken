# Godwoken Configurations

## How Godwoken locates config file

Godwoken looks for configuration file <config.toml> in current working directory by default.
```sh
> ./target/release/godwoken run -h

godwoken-run 
Run Godwoken node

USAGE:
    godwoken run [FLAGS] -c <config>

FLAGS:
    -h, --help                 Prints help information
        --skip-config-check    Force to accept unsafe config file
    -V, --version              Prints version information

OPTIONS:
    -c <config>        The config file path [default: ./config.toml]
```
Command line argument `-c <config_file_path>` sets the value of config file path.

## Generate an example config file
```sh
> godwoken generate-example-config
```
This command will generate `config.example.toml`.
Then edit the generated config files according to the in-line comments.

## Default configs
```toml
[mem_pool]
# An error will occur when the consumed cycles exceed the `max_cycles` limit.

# Maximum allowed cycles to execute a transaction came from
# `execute_l2transaction` or `execute_raw_l2transaction` RPC interface
execute_l2tx_max_cycles = 100000000
# Maximum allowed cycles to execute a transaction came from
# `submit_l2transaction` RPC interface
submit_l2tx_max_cycles  = 90000000
```
