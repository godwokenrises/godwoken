# Changelog
Documentation of all notable changes to the Godwoken related projects.

The format is based on [Keep a Changelog](https://keepachangelog.com).

## [Unreleased]

## [v1.12.0] - 2023-02-27

* config(mainnet.toml): bump Polyjuice backend to v1.5.3 [#1000](https://github.com/godwokenrises/godwoken/pull/1000)

## [v1.12.0-rc2] - 2023-02-21

* config: add new fork for testnet at block#1916000 [#993](https://github.com/godwokenrises/godwoken/pull/993)
* fix(withdrawal_unlocker): remove dry_run_transaction and fix error display [#992](https://github.com/godwokenrises/godwoken/pull/992)

## [v1.12.0-rc1] - 2023-02-08

The `web3` and `web3-indexer` components have been added to the monorepo since this release, and we bumped the version from `v1.8.x` to `v1.12.x` to unify the version.

We introduced a breaking change of the config file in [#946](https://github.com/godwokenrises/godwoken/pull/946). The consensus-related options are moved into option `consensus`, and we put the builtin `testnet` and `mainnet` consensus into the godwoken program. This change reduces the operation works of node maintainers.

Highlights:

- Builtin mainnet and testnet consensus config [#946](https://github.com/godwokenrises/godwoken/pull/946)
- Add web3 and web3-indexer into the monorepo [#910](https://github.com/godwokenrises/godwoken/pull/910)

Bug fixes:

- gwos-evm: fix big mod exp [#932](https://github.com/godwokenrises/godwoken/pull/932)
- fix(web3): eth_getFilterLogs should return all matched logs [#947](https://github.com/godwokenrises/godwoken/pull/947)

Enhenchment:

- chore: use TransactionDB and refactor store [#903](https://github.com/godwokenrises/godwoken/pull/903)
- chore: refactor rpc server [#927](https://github.com/godwokenrises/godwoken/pull/927)
- Improve the error code of API when executing transactions [#930](https://github.com/godwokenrises/godwoken/pull/930)

## [v1.8.0-rc2] - 2022-12-19

A major change in this release is re-interpreting the meaning of the `xxx_timepoint` field to `finalized timestamp`. 
Thus, we can use the CKB transaction's `since` field to determine the l1 timestamp and to unlock l1 cells without reference to the Rollup cell. It also simplifies the finality determination of withdrawal cells.

- feat: change timepoint interpretation [#897](https://github.com/godwokenrises/godwoken/pull/897)
- refactor: rename structure fields [#912](https://github.com/godwokenrises/godwoken/pull/912)

We also adjust the documentation:

- doc: update Finality Mechanism Changes [#913](https://github.com/godwokenrises/godwoken/pull/913/files)

Other changes:

- refactor: move gw-types and gw-common to gwos folder [#905](https://github.com/godwokenrises/godwoken/pull/905)
- feat: support CKB built-in indexer [#907](https://github.com/godwokenrises/godwoken/pull/907)

## [v1.8.0-rc1] - 2022-12-09

In this version, an upgrading of on-chain scripts is included:

- feat: optimize Godwoken finality mechanism [#836](https://github.com/godwokenrises/godwoken/pull/836)
- feat: deprecate verifications for state_checkpoint_list and prev_state_checkpoint [#883](https://github.com/godwokenrises/godwoken/pull/883)

We also introduce a change to activate the new behavior.

- feat: determine global state version according to fork height[#858](https://github.com/godwokenrises/godwoken/pull/858)

Experimental gas-less feature [(discussion link)](https://github.com/godwokenrises/godwoken/discussions/860):

- feat: (optionally) support gasless transactions [#869](https://github.com/godwokenrises/godwoken/pull/869)

Other changes:

- perf: optional SMT trie feature and migrate command [#859](https://github.com/godwokenrises/godwoken/pull/859)
- feat: optimized trace and metrics [#865](https://github.com/godwokenrises/godwoken/pull/865)
- fix(withdrawal): finalized withdrawal take longer time to unlock [#892](https://github.com/godwokenrises/godwoken/pull/892)
- chore(CI): add docker-prebuilds into monorepo [#885](https://github.com/godwokenrises/godwoken/pull/885)
- feat: support non-x86 build [#882](https://github.com/godwokenrises/godwoken/pull/882)

## [v1.7.3] - 2022-11-27

- config: deny unknown fields in the config toml file [#862](https://github.com/godwokenrises/godwoken/pull/862)
- Cherry pick commits from develop branch to fix CI script tests [#878](https://github.com/godwokenrises/godwoken/pull/878)

## [v1.7.2] - 2022-11-25

- fix: use mem pool state for “get block” RPCs [#871](https://github.com/godwokenrises/godwoken/pull/871)

## [v1.7.1] - 2022-11-13

- fix: Support revert inner call state [#835](https://github.com/godwokenrises/godwoken/pull/835)
- refactor(monorepo): Add godwoken-scripts [#839](https://github.com/godwokenrises/godwoken/pull/839) and godwoken-polyjuice [#849](https://github.com/godwokenrises/godwoken/pull/849) to monorepo
- fork(consensus): Increase l2 tx max cycles from 150M to 500M [#852](https://github.com/godwokenrises/godwoken/pull/852)

## [v1.7.0] - 2022-11-03

- fix(pool): insert re-injected withdrawals to db [#828](https://github.com/godwokenrises/godwoken/pull/828)
- fix(tools): fee rate is 0 in config file generated by tools [#830](https://github.com/godwokenrises/godwoken/pull/830)
- fix(mem-pool): remove re-injected failed txs in mem pool [#831](https://github.com/godwokenrises/godwoken/pull/831)
- feat: add rewind-to-last-valid-block subcommand [#832](https://github.com/godwokenrises/godwoken/pull/832)
- fix: delete withdrawal info when detach block [#833](https://github.com/godwokenrises/godwoken/pull/833)
- fix: check block size and retry if too large [#834](https://github.com/godwokenrises/godwoken/pull/834)

## [v1.7.0-rc2] - 2022-10-25

- fix(psc): don't revert if transaction input is consumed by itself [#819](https://github.com/nervosnetwork/godwoken/pull/819)

## [v1.7.0-rc1] - 2022-09-26

- Decouple block producing, submission and confirming [#776](https://github.com/nervosnetwork/godwoken/pull/776)

## [v1.6.2] - 2022-10-25

- Increase max return data [#822](https://github.com/godwokenrises/godwoken/pull/822)

## [v1.6.1] - 2022-10-18

- Add `fee_rate` option to block_producer config [#815](https://github.com/godwokenrises/godwoken/pull/815)

## [v1.6.0] - 2022-09-13


- Support non EIP-155 transaction [#777](https://github.com/nervosnetwork/godwoken/pull/777)
- Fix withdrawal command in cli [#792](https://github.com/nervosnetwork/godwoken/pull/792)
- Support native token transfer [#788](https://github.com/nervosnetwork/godwoken/pull/788)

## [v1.5.0] - 2022-08-12

- fix(mempool): pool cycles not reset on next mem block for readonly node [#781](https://github.com/nervosnetwork/godwoken/pull/781)

## [v1.5.0-rc1] - 2022-08-09

- fix(tests): wait withdrawal pushed into mem pool [#774](https://github.com/nervosnetwork/godwoken/pull/774)
- Add RPC get_pending_tx_hashes [#772](https://github.com/nervosnetwork/godwoken/pull/772)
- feat: Introduce max_cycles_limit of a Godwoken block [#767](https://github.com/nervosnetwork/godwoken/pull/767)

## [v1.4.0-rc4] - 2022-07-26

- hotfix(rpc server): submit withdrawal missing data for submit_tx [#770](https://github.com/nervosnetwork/godwoken/pull/770)

## [v1.4.0-rc3] - 2022-07-21

- fix(config): optional block producer wallet for readonly node [#768](https://github.com/nervosnetwork/godwoken/pull/768)

## [v1.4.0-rc2] - 2022-07-19

- fix(rpc): calculate tx signature hash using packed bytes [#760](https://github.com/nervosnetwork/godwoken/pull/760)

## [v1.4.0-rc1] - 2022-07-14

- Automatically create account for undeposited sender [#710](https://github.com/nervosnetwork/godwoken/pull/710)
- Check sender's balance in `execute_raw_l2transaction` RPC [#750](https://github.com/nervosnetwork/godwoken/pull/750)
- Add export and import block command [#754](https://github.com/nervosnetwork/godwoken/pull/754)
- Fix gw-tools `stat-custodian-ckb` command [#757](https://github.com/nervosnetwork/godwoken/pull/757)
- Redirect layer 2 transaction syscall log to sentry [#758](https://github.com/nervosnetwork/godwoken/pull/758)

## [v1.3.0-rc2] - 2022-07-19

- fix(withdrawal unlocker): unhandle tx status unknown and rejected [#764](https://github.com/nervosnetwork/godwoken/pull/764)

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
