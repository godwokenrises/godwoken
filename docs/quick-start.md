# Quick Start

## Requirements

- Install [ckb](https://github.com/nervosnetwork/ckb)(v0.100.0 or above), run ckb and ckb-miner
- Install [ckb indexer](https://github.com/nervosnetwork/ckb-indexer)(v0.3.0 or above) and run
- Install [ckb_cli](https://github.com/nervosnetwork/ckb-cli)(v0.100.0 or above)
- Install [moleculec](https://github.com/nervosnetwork/molecule)(v0.7.2)
- Install [capsule](https://github.com/nervosnetwork/capsule)(v0.4.6)

## Clone the source with git

```bash
git clone --recursive https://github.com/nervosnetwork/godwoken
cd godwoken
```

## Setup

We can use gw-tools `setup` command to complete settings: building scripts, deploy scripts, initialize layer2 genesis block, and generate configurations.

Before that, we need to prepare a deploy key with enough CKB(about 2 millions for the default setup).

```bash
gw-tools setup -n 2 -k <deploy_key> --scripts-build-config build-scripts.json -c setup-config.json
```

The input file `scripts-build.json` describes how we build CKB scripts.

```json
{
    "prebuild_image": "nervos/godwoken-prebuilds:v0.6.7",
    "repos": {
        "godwoken_scripts": "https://github.com/nervosnetwork/godwoken-scripts#master",
        "godwoken_polyjuice": "https://github.com/nervosnetwork/godwoken-polyjuice#main",
        "clerkb": "https://github.com/nervosnetwork/clerkb#v0.4.0"
    }
}
```

**NOTES**: By default, the setup command is executed in `build` mode. You can specify the `copy` mode with the additional parameter `-m copy`, then the deployment process will copy the scripts from prebuilt docker image instead of building it, which saves a lot of time.

The another input file `setup-config.json` provides several configures for the Rollup.

``` json
{
  "l1_sudt_script_type_hash": "0xc5e5dcf215925f7ef4dfaf5f4b4f105bc321c02776d6e7d52a1db3fcd9d011a4",
  "l1_sudt_cell_dep": {
    "dep_type": "code",
    "out_point": {
      "tx_hash": "0xe12877ebd2c3c364dc46c5c992bcfaf4fee33fa13eebdf82c591fc9825aab769",
      "index": "0x0"
    }
  },
  "node_initial_ckb": 1200000,
  "cells_lock": {
    "code_hash": "0x49027a6b9512ef4144eb41bc5559ef2364869748e65903bd14da08c3425c0503",
    "hash_type": "type",
    "args": "0x0000000000000000000000000000000000000000"
  },
  "reward_lock": {
    "code_hash": "0x49027a6b9512ef4144eb41bc5559ef2364869748e65903bd14da08c3425c0503",
    "hash_type": "type",
    "args": "0x0000000000000000000000000000000000000000"
  },
  "burn_lock": {
    "code_hash": "0x0000000000000000000000000000000000000000000000000000000000000000",
    "hash_type": "data",
    "args": "0x"
  }
}
```

**NOTES**: You must modify this file, to set correct simple UDT script, otherwise the sUDT deposit won't work. The `cells_lock` is used to unlock/upgrade Rollup scripts. `reward_lock` is used to receive challenge rewards. The `burn_lock` is used to received burned assets should be unlock-able.

## Start Node

Now you can adjust the `config.toml` file and start godwoken node.

```bash
cd output/node1
cp -r ../scripts ./scripts
./godwoken run
```

**NOTES**: 

- The default node mode is `readonly`, which can be modified to `fullnode` mode or `test` mode in config.toml.
- If you need to start multiple nodes in the same environment, you can manually modify the listening port number in their respective config.toml.
