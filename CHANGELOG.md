# Changelog
Documentation of all notable changes to the Godwoken related projects.

The format is based on [Keep a Changelog](https://keepachangelog.com).


## [Unreleased]

## [1.1.0] - 2022-04-2x (release candidate)

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
