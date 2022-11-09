# Comparison with EVM

Polyjuice aims at 100% EVM compatibility as a goal, meaning we plan to support all smart contracts supported by the latest Ethereum hardfork version. But in the current version, something is incompatible with EVM.

## EVM revision
The maximum EVM revision supported is `EVMC_BERLIN`.

## pCKB

[pCKB](https://github.com/nervosnetwork/godwoken/blob/develop/docs/life_of_a_polyjuice_transaction.md#pckb) is a new concept introduced by Polyjuice.

Recall that in Ethereum, the gas of each smart contract is calculated. The transaction fee is calculated then by multiplying gas with specified gas price. In Polyjuice, **pCKB** is used as the unit for calculating transaction fees. This means while the gas price in Ethereum is ETH/gas(which is denominated in wei, which is 1e-18 ether), in Polyjuice gas price is measured in pCKB/gas. When executing a transaction, Polyjuice will deduct transaction fee using the layer-2 [sUDT](https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0025-simple-udt/0025-simple-udt.md) type denoted by **pCKB**.

Note when sending a transaction to a smart contract for certain behavior, the `value` of the transaction is `pCKB`.

## sUDT-ERC20 proxy contract

When you use a sUDT token type, it will be represented in Godwoken as a layer-2 sUDT type. Polyjuice ensures that all the layer-2 sUDT tokens on Godwoken are in compliance with the ERC20 standard by the [sUDT-ERC20 Proxy Contract](../solidity/erc20/README.md). This contract provides a way for EVM code to interact with ERC20 standard interface to operate sUDT tokens on Godwoken as if they were ERC20 tokens.

In other words, all bridged sUDT tokens have the same ERC20 interface thanks to the 1-to-1 sUDT-ERC20 proxy contract:

### Bridged sUDT token list
- mainnet_v1: https://github.com/nervosnetwork/godwoken-info/blob/main/mainnet_v1/bridged-token-list.json
- testnet_v1: https://github.com/nervosnetwork/godwoken-info/blob/main/testnet_v1_1/bridged-token-list.json

## Transaction structure

A Polyjuice transaction is essentially just a Godwoken transaction.

When you send an Ethereum transaction, the transaction is converted to Godwoken [RawL2Transaction](https://github.com/nervosnetwork/godwoken/blob/v1.5.0/crates/types/schemas/godwoken.mol#L69-L76) type which is automatically handled by [Godwoken Web3](https://github.com/nervosnetwork/godwoken-web3/tree/v1.6.4).

## Behavioral differences of some opcodes

| EVM Opcode | Solidity Usage     | Behavior in Polyjuice         | Behavior in EVM                      |
| ---------- | ------------------ | ----------------------------- | ------------------------------------ |
| COINBASE   | `block.coinbase`   | address of the block_producer | address of the current block's miner |
| GASLIMIT   | `block.gaslimit`   | 12,500,000                    | current block's gas limit            |
| DIFFICULTY | `block.difficulty` | 2,500,000,000,000,000         | current block's difficulty           |

## Restriction of memory usage

Polyjuice runs EVM on [ckb-vm](https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0003-ckb-vm/0003-ckb-vm.md#risc-v-runtime-model). While EVM has no limit on memory usage (despite the limit of 1024 on stack depth for EVM), ckb-vm can use a maximum of 4MB of memory for now. Of which, 3MB for heap space and 1MB for stack space. See more details in [here](https://github.com/nervosnetwork/riscv-newlib/blob/00c6ae3c481bc62b4ac016b3e86c508cdf2e68d2/libgloss/riscv/sys_sbrk.c#L38-L56). 

For some contracts that consume a lot of memory or that have deep call stacks, this may indicate a potential incompatibility on ckb-vm.

## Others

* Transaction context
  * `chain_id` is defined in Godwoken [RollupConfig#chain_id](https://github.com/nervosnetwork/godwoken/blob/v1.5.0/crates/types/schemas/godwoken.mol#L64).
  * the block difficulty is always `2500000000000000`
  * the gas limit for each block is 12500000; it is not a transaction-level limit. Any transaction can reach the gas limit
  * the size limit for contract's return data is [`25KB`](https://github.com/nervosnetwork/godwoken-scripts/blob/31293d1/c/gw_def.h#L21-L22)
  * the size limit for contract's storage is [`25KB`](https://github.com/nervosnetwork/godwoken-scripts/blob/31293d1/c/gw_def.h#L21-L22)

* `transaction.to` MUST be a Contract Address

* The `transfer value` can not exceed `uint128:MAX`, because the type of [sUDT.amount](https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0025-simple-udt/0025-simple-udt.md#sudt-cell) is `uint128`

* Pre-compiled contract
  * [addition pre-compiled contracts](Addition-Features.md)
