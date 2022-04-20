This release note includes the new features and major updates in Godwoken v1.

> Notice, the Godwoken v1 is not an upgrade on the original chain! we will deploy v1 as a new chain, and provide tools to help users and developers migrating to the new chain.

## Compatiblility improvements

In the new version, we improve the compatibility of Godwoken:

- Provide API level compatible. Deprecate web3-provider plugin.
- Support native ETH address in API and EVM, remove the Godwoken address concept.
- Support Ethereum signature format and EIP-712. User can view the transaction before signing, instead of signing a random 32 bytes message. [#561](https://github.com/nervosnetwork/godwoken/pull/561)
- Fix the total supply interface of sUDT ERC-20 contract [#560](https://github.com/nervosnetwork/godwoken/pull/560)
- Support interactive with eth address that haven't been registered to Godwoken.

In short, as a developer, you can use Godwoken v1 just like anyother Ethereum compatible chain, all you need to do is to switch the network to Godwoken. The polyjuice-provider web3 plugin is removed in the v1 version.

## Other improvements

- Support p2p mem-pool syncing [#642](https://github.com/nervosnetwork/godwoken/pull/642), we need further PRs to enable fully decentralized syncing, but this PR is a good starting.
- perf: optimize molecule usage [#640](https://github.com/nervosnetwork/godwoken/pull/640)
- perf: use BTreeSet in FeeQueue [#641](https://github.com/nervosnetwork/godwoken/pull/641)
- Change rollup cell's lock to omni-lock [#608](https://github.com/nervosnetwork/godwoken/pull/608), Due to an error of secp256k1 lock, we can't fill to many data in the witness field of CKB transaction, this PR enable the rollup to submits larger size block.

## Godwoken internal changes

> If you are a Dapp developer, feel free to skip it and move on.

We add a new concept: Ethereum address registry to store ethereum address in Godwoken. Godwoken creates a mapping between Ethereum address and account once user deposit to a new account. We also adjust few RPC to support ethereum address as the parameter. A few Godwoken data structure is adjusted to support new address format.

You can learn more details about Godwoken internal changes in: [docs/release-notes/v1-internal-CHANGES.md](https://github.com/nervosnetwork/godwoken/blob/develop/docs/release-notes/v1-internal-CHANGES.md)
