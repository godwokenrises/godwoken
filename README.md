# Godwoken

Welcome to the Godwoken Monorepo! This repository is the home of the Godwoken project.

Godwoken is a key component of Nervos' layer-2, working in conjunction with the Rollup technique to create a scalable, EVM-compatible layer for the Nervos network. With Godwoken, developers can build decentralized applications (dapps) on the Ethereum Virtual Machine (EVM) and run them on the Nervos network, increasing the scalability and efficiency of the network.

Godwoken provides a solution for dApp builders who seek the advantages of Ethereum - Like its community, good tools, and documentation - but not the drawbacks that Ethereum faces, such as network congestion, high gas fees, oversaturation and scalability issues.


## Repository Structure

This monorepo contains all the packages and libraries necessary to develop and run Godwoken. The packages are organized into the following directories:

- `crates`: Contains the Rust code for Godwoken node. This is where developers can find the main Godwoken offchain code.
- `web3`: Web3 adapter layer, contains a web server providing web3 compatible RPC, and a block indexing program.
- `gwos`: 
    - `contracts`: Contains layer-1 contracts used to verify Godwoken layer-2 state transition.
    - `c`: The layer-2 builtin contraints and layer-2 syscalls.
- `gwos-evm`: The EVM layer on Godwoken, itself is a Godwoken layer-2 contract.

## Understanding the Components

Godwoken is comprised of several key components that work together to provide a scalable, EVM-compatible layer-2 solution for Nervos. Here's a brief overview of each component:

### Godwoken Node

The code in the `generator`, `mem-pool`, `block-producer` under the `/crates` directory are the backbone of Godwoken node. They define the logic and rules for the layer-2 node behavior, including the mechanism for submitting and processing transactions.

The `/crates/chain` contains an implement of the syncing mechanism that syncing layer-1 blocks from CKB's RPC.

The `/crates/challenge` contains WIP challenger implementation, which is a core part of the optimistic rollup. The challenger will be reimplemented with the interactive challenge technique to reduce the on-chain challenge cost. Until then, the Godwoken network is [half centralized](https://docs.godwoken.io/overview#decentralization-roadmap).

### GWOS scripts

GWOS contains CKB scripts under `/gwos/contracts`. These scripts run upon layer-1 to verify the Godwoken layer-2 state transition.

Godwoken's layer-2 state is compressed into a sparse-merkle-tree which is stored in a CKB cell. Read [GWOS Readme](gwos/README.md) to learn how GWOS scripts work.

### Layer-2 contracts

Layer-2 contracts are running upon Godwoken. A layer-2 contract can modify Godwoken's sparse-merkle-tree state via syscalls.

The `gwos/c/gw_def.h` contains syscall interfaces write in C. and `/gwos/c/contracts` contains three builtin layer-2 contracts.

Technically, the layer-2 contracts can be written in any programming language that can be compiled to RISC-V thanks to [CKB VM](https://github.com/nervosnetwork/ckb-vm)'s ability. But currently, Godwoken only provides an EVM programming interface for layer-2 developers.

### EVM

`/gwos-evm` contains an EVM implementation that can run upon CKB-VM, which itself is also a layer-2 contract.

## Public Networks
- [Godwoken Mainnet v1](https://docs.godwoken.io/connectionInfo#godwoken-mainnet-v1)
- [Godwoken Testnet v1](https://docs.godwoken.io/connectionInfo#godwoken-testnet-v1)

## Documentation
- https://docs.godwoken.io
- [Ethereum Compatible Web3 RPC](web3/docs/apis.md)

## Getting Started

### Start a local dev chain

[Godwoken kicker](https://github.com/godwokenrises/godwoken-kicker/blob/develop/docs/kicker-start.md) is a recommended tool to start a local dev chain inside docker containers.

### Start a read-only node

We put mainnet and testnet configs under [godwoken-info](https://github.com/godwokenrises/godwoken-info), you can run a Godwoken node with these config to start syncing with the testnet and the mainnet.

## License

Godwoken is open-source software licensed under the MIT license.
