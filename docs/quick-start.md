# Quick Start

## Requirements

- Install [ckb](https://github.com/nervosnetwork/ckb)(v0.40.0 or above), run ckb and ckb-miner
- Install [ckb indexer](https://github.com/nervosnetwork/ckb-indexer)(v0.2.1 or above) and run
- Install [ckb_cli](https://github.com/nervosnetwork/ckb-cli)(v0.40.0 or above)
- Install [moleculec](https://github.com/nervosnetwork/molecule)(v0.6.1 or above)
- Install [capsule](https://github.com/nervosnetwork/capsule)(v0.4.6 or above)

## Clone the source with git

```bash
git clone --recursive https://github.com/nervosnetwork/godwoken
cd godwoken
```

## Setup

The setup subcommand in gw-tools crate can complete all the settings before the godwoken node starts: preparing scripts, deploying scripts, deploying layer 2 genesis block, and generating configuration files. The command can be used as follows:

```bash
RUST_LOG=info cargo +nightly run --bin gw-tools -- setup -s deploy/scripts-build.json -k deploy/pk -o deploy/
```

The input file scripts-build.json for this command is as follows(you need to modify the prebuild_image & repos):

```json
{
    "prebuild_image": "nervos/godwoken-prebuilds:v0.5.0-rc2-with-debugger",
    "repos": {
        "godwoken_scripts": "https://github.com/nervosnetwork/godwoken-scripts#master",
        "godwoken_polyjuice": "https://github.com/nervosnetwork/godwoken-polyjuice#main",
        "clerkb": "https://github.com/nervosnetwork/clerkb#v0.4.0"
    }
}
```

**NOTES**: By default, the setup command is executed in `build` mode. You can specify the `copy` mode with the additional parameter `-m copy` to copy the precompiled scripts  from prebuilt docker image, which can save a lot of time to complete the setup process..

After the setup command is successfully completed, you need to fill in the custom reward lock information in the node's config.toml(default relative path: deploy/node1/config.toml):

```toml
[block_producer.challenger_config.rewards_receiver_lock]
code_hash = '<code_hash>'
hash_type = 'type'
args = '0x'
```

## Start Node

Now you can start godwoken node.

```bash
RUST_LOG=info cargo +nightly run --bin godwoken run -c deploy/node1/config.toml
```

**NOTES**: 

- The default node mode is `readonly`, which can be modified to `fullnode` mode or `test` mode in config.toml.
- If you need to start multiple nodes in the same environment, you can manually modify the listening port number in their respective config.toml.
