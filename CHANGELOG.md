# Changelog
Documentation of all notable changes to the Godwoken related projects.

The format is based on [Keep a Changelog](https://keepachangelog.com).

## [Unreleased]

- Automatically create account for undeposited sender [#710](https://github.com/nervosnetwork/godwoken/pull/710)

## [v1.3.0-rc1] - 2022-07-01

- Fix gw-tools `transfer` command [#744](https://github.com/nervosnetwork/godwoken/pull/744)
- Fix gw-tools `create-sudt-account` command [#747](https://github.com/nervosnetwork/godwoken/pull/747)
- Return error if withdrawal capacity is lower than minimal withdrawal capacity [#748](https://github.com/nervosnetwork/godwoken/pull/748)
- Check sender's balance before execute transaction [#750](https://github.com/nervosnetwork/godwoken/pull/750)

## [v1.2.2] - 2022-06-20
- Improve block producer chain task tracing [#732](https://github.com/nervosnetwork/godwoken/pull/732)
- fix: readonly nodes without p2p sync [#737](https://github.com/nervosnetwork/godwoken/pull/737)

## [v1.2.1-rc1] - 2022-06-15
- Refresh readonly mem-pool when receives new mem-block [#721](https://github.com/nervosnetwork/godwoken/pull/721)
- fix: reject transactions has less gas than the intrinsic gas [#725](https://github.com/nervosnetwork/godwoken/pull/725)

## [v1.2.0-rc1] - 2022-06-11

- Support packaging failed transactions into layer2 block [#684](https://github.com/nervosnetwork/godwoken/pull/684)
- Support upgrade backend executable binaries [#713](https://github.com/nervosnetwork/godwoken/pull/713)
- Support new option `max_txs`, `max_deposits` and `max_withdrawals` in config file, these options controls the maximum number of each items in a block [#714](https://github.com/nervosnetwork/godwoken/pull/714)
- Return committed info on withdrawal query RPC [#706](https://github.com/nervosnetwork/godwoken/pull/706)

## [v1.1.4] - 2022-05-30
- Improve the withdrawal packaging performance [#701](https://github.com/nervosnetwork/godwoken/pull/701)
- Improve the performance of deposits [#703](https://github.com/nervosnetwork/godwoken/pull/703)

## [v1.1.0-beta](https://github.com/nervosnetwork/godwoken-docker-prebuilds/pkgs/container/godwoken-prebuilds/21567994?tag=v1.1.0-beta) - 2022-05-08

> Note that Godwoken v1 is not an upgrade on the existing chain! Instead, v1 will be deployed as a new chain with tools to help users and developers migrate to the new chain.

### Ethereum Compatiblility Improvements

In the new version, compatibility improvements for Godwoken include:

- Provide API level 
compatibility, remove the web3-provider plugin.
- Support native ETH address in API and EVM, remove the Godwoken address concept.
- Support Ethereum signature format and EIP-712. User can view the transaction before signing, instead of signing a random 32 bytes message. [#561](https://github.com/nervosnetwork/godwoken/pull/561)
- Fix the `totalSupply` interface of sUDT ERC-20 proxy contract [#560](https://github.com/nervosnetwork/godwoken/pull/560)
- Support interactive with eth address that hasn't been registered to Godwoken.
- Unify layer 2 fungible token represatation as uint256.
- Change layer 2 ckb decimal from 8 to 18, improve compatibility between metamask and native ckb. [#675](https://github.com/nervosnetwork/godwoken/pull/675)

Developers can use Godwoken v1 the same way they use other ethereum-compatible chains, requiring only switching the network to Godwoken. The polyjuice-provider web3 plugin was removed in Godwoken v1.

### Other improvements

- Support p2p mem-pool syncing [#642](https://github.com/nervosnetwork/godwoken/pull/642), further PRs are needed to enable fully decentralized syncing, but this PR is a good starting.
- perf: optimize molecule usage [#640](https://github.com/nervosnetwork/godwoken/pull/640)
- perf: use BTreeSet in FeeQueue [#641](https://github.com/nervosnetwork/godwoken/pull/641)
- Change rollup cell's lock to omni-lock [#608](https://github.com/nervosnetwork/godwoken/pull/608). This PR enables the optimistic rollup to submit larger blocks to fix the inability of putting too much data in the witness field of a CKB transaction due to a secp256k1-lock limit.

### Godwoken internal changes

> If you are a Dapp developer, feel free to skip it and move on.

v1 adds a new concept in having the Ethereum address registry stores Ethereum addresses in Godwoken. Once user deposits a new account, Godwoken will create a mapping between the Ethereum address and the account script hash. In addition, some RPCs have been adapted to support Ethereum addresses as parameters, and some Godwoken data structures have been adapted to support the new address format.

More details about Godwoken internal changes refer to: [docs/v1-release-note.md](https://github.com/nervosnetwork/godwoken/blob/develop/docs/v1-release-note.md)
