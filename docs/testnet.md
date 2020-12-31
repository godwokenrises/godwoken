We now have a deployed Godwoken instance running on CKB Aggron4 testnet using PoA as block issuance mechanism. You can setup your own Godwoken instance to connect to the testnet one in readonly mode. Readonly mode here means:

* You can sync new layer 2 blocks as a typical Godwoken instance.
* You can query for on chain states, such as account balance, storage state, etc.
* You can execute a smart contract using current tip state.

But you will not be able to submit layer 2 transactions via a readonly mode. Note this is just a limitation at the moment, since Godwoken now relies solely on layer 1 CKB for syncing. One future plan of Godwoken, includes building a proper layer 2 transaction pool, after which, a readonly mode node would then be able to submit layer 2 transactions.

## Initial Setup

First, initialize a CKB testnet chain:

```bash
$ export TOP=$(pwd)
$ ckb init -c testnet -C testnet
$ ckb run -C testnet
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
$ cd lumos
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

## Start Godwoken in Readonly Mode

```bash
$ cd $TOP
$ git clone --recursive https://github.com/nervosnetwork/godwoken
$ cd godwoken
$ cd c
$ make all-via-docker
$ cd ..
$ cargo build
$ yarn
$ curl -LO https://raw.githubusercontent.com/nervosnetwork/godwoken-examples/21ab518f2b73f83e1f13350b958f20763dee195f/packages/demo/src/configs/testnet_config.json
$ yarn workspace @ckb-godwoken/base tsc
$ yarn workspace @ckb-godwoken/runner tsc
$ LUMOS_CONFIG_NAME=AGGRON4 node packages/runner/lib/index.js -c testnet_config.json -s "postgresql://user:password@127.0.0.1:5432/lumos"
```

Notice due to lumos syncing, the initial startup might take a long time, we are actively working to optimize this.
