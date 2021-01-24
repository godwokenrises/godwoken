This document explains how you can setup your own godwoken instance locally on a dev chain. You might notice here we use always success contracts extensively here:

* Godwoken related contracts are all replaced with always success contracts.
* Always success is used instead of PoA or PoS to control block issuance.

## Initial Setup

First, initialize a CKB chain:

```bash
$ export TOP=$(pwd)
$ ckb init -c dev -C godwoken-test1
```

Make modifications to ckb config files to setup miners. I typically modify [genesis message](https://github.com/nervosnetwork/ckb/blob/624169510b93ce4bc029d0dd502c86bccd5435a4/resource/specs/dev.toml#L12) so I get a different chain each time, but this is not required.

Now setup CKB:

```bash
$ ckb run -C godwoken-test1
```

In a different terminal:

```bash
$ ckb miner -C godwoken-test1
```

For now, we use dummy always success contracts as contract placeholders. You can of course try out real contracts, but here we are opting for the simpler way:

```bash
$ cd $TOP
$ cat << EOF > always_success.S
  .global _start
_start:
  li a7, 93
  ecall
EOF
$ sudo docker run --rm -it -v `pwd`:/code nervos/ckb-riscv-gnu-toolchain:xenial bash
(docker) $ cd /code
(docker) $ riscv64-unknown-elf-gcc -o deposition_lock always_success.S -nostartfiles -nostdlib
(docker) $ riscv64-unknown-elf-gcc -o custodian_lock always_success.S -nostartfiles -nostdlib
(docker) $ riscv64-unknown-elf-gcc -o withdrawal_lock always_success.S -nostartfiles -nostdlib
(docker) $ riscv64-unknown-elf-gcc -o state_validator_lock always_success.S -nostartfiles -nostdlib
(docker) $ riscv64-unknown-elf-gcc -o state_validator_type always_success.S -nostartfiles -nostdlib
(docker) $ exit
$ cat << EOF > deployment.json
{
  "programs": {
    "deposition_lock": "deposition_lock",
    "custodian_lock": "custodian_lock",
    "withdrawal_lock": "withdrawal_lock",
    "state_validator_lock": "state_validator_lock",
    "state_validator_type": "state_validator_type"
  },
  "lock": {
    "code_hash": "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
    "hash_type": "type",
    "args": "0x2aa5cde8b19d02709861406cac02ce324a7ea31d"
  }
}
EOF
```

You can adjust `deployment.json` file with the lock you want to use.

Generate lumos config file using [lumos-config-generator](https://github.com/classicalliu/lumos-config-generator):

```bash
$ cd $TOP
$ lumos-config-generator lumos-config.json
```

Note you need to add a `ANYONE_CAN_PAY` script to lumos config file, this is due to lumos' bug. The actual script configuration does not matter. We won't use ACP here.

Now clone godwoken code:

```bash
$ cd $TOP
$ git clone --recursive https://github.com/nervosnetwork/godwoken
$ cargo install moleculec --version 0.6.1
$ cd godwoken
$ cd c
$ make all-via-docker
$ cd ..
$ cargo build
$ yarn
```

## Database Setup

Create a postgres instance:

```bash
$ docker run --name postgres -e POSTGRES_USER=user -e POSTGRES_DB=lumos -e POSTGRES_PASSWORD=password -d -p 5432:5432 postgres
```

Clone lumos so we can initialize the database:

```bash
$ cd $TOP
$ git clone --recursive https://github.com/nervosnetwork/lumos
$ cd lumos && git checkout v0.14.2-rc6
$ yarn
$ cd packages/sql-indexer
$ cat << EOF > knexfile.js
module.exports = {
  development: {
    client: 'postgresql',
    connection: {
      database: 'lumos',
      user:     'user',
      password: 'password'
    },
    pool: {
      min: 2,
      max: 10
    },
    migrations: {
      tableName: 'knex_migrations'
    }
  }
};
EOF
$ npx knex migrate:up
```

## Deploying Contracts

```bash
$ cd $TOP/godwoken
$ yarn workspace @ckb-godwoken/base tsc
$ yarn workspace @ckb-godwoken/tools tsc
$ LUMOS_CONFIG_FILE=$TOP/lumos-config.json node packages/tools/lib/deploy_scripts.js --private-key <private key used to deploy contracts> -f $TOP/deployment.json -o $TOP/deployment-results.json -s postgresql://user:password@localhost:5432/lumos
```

## Create Genesis Block

First, create a godwoken config file:

```bash
$ cd $TOP
$ cat << EOF > godwoken_config.json
{
  "genesis": {
    "timestamp": "0x1234"
  },
  "store": {
    "path": "chain-data"
  }
}
EOF
```

```bash
$ cd $TOP/godwoken
$ yarn workspace @ckb-godwoken/base tsc
$ yarn workspace @ckb-godwoken/tools tsc
$ LUMOS_CONFIG_FILE=$TOP/lumos-config.json node packages/tools/lib/deploy_genesis.js --private-key <private key used to create genesis block> -d $TOP/deployment-results.json -c $TOP/godwoken_config.json -o $TOP/runner_config.json -s "postgresql://user:password@127.0.0.1:5432/lumos"
```

## Stake

Anyone can stake to becom an aggregator:

```
$ cd $TOP/godwoken
$ LUMOS_CONFIG_FILE=$TOP/lumos-config.json node packages/runner/lib/stake.js --private-key <private key for aggregator> -f $TOP/runner_config.json --capacity 1000 -s "postgresql://user:password@127.0.0.1:5432/lumos"
```

And unstake to withdraw fund after `finalized block number`:
```
$ cd $TOP/godwoken
$ LUMOS_CONFIG_FILE=$TOP/lumos-config.json node packages/runner/lib/unstake.js --private-key <private key for aggregator> -f $TOP/runner_config.json -s "postgresql://user:password@127.0.0.1:5432/lumos"
```

## Add Sentry Support

By adding `sentryConfig` to `$TOP/runner_config.json`, you can upload error logs to your sentry service: 
```
{
  ...
  "sentryConfig": {
    "dsn": ${your_dsn},
    "tracesSampleRate": 1
  }
}
```
Find `your_dsn` via this [guide](https://docs.sentry.io/product/sentry-basics/dsn-explainer/). 

You can also leave it unchanged if you don't need sentry support.
## Start Godwoken

```bash
$ cd $TOP/godwoken
$ yarn workspace @ckb-godwoken/base tsc
$ yarn workspace @ckb-godwoken/runner tsc
$ LUMOS_CONFIG_FILE=$TOP/lumos-config.json node packages/runner/lib/index.js --private-key <private key for aggregator> -c $TOP/runner_config.json -s "postgresql://user:password@127.0.0.1:5432/lumos"
```
