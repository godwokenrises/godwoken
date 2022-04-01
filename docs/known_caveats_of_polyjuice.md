# Known Caveats Of Polyjuice

When designing and building Polyjuice, we aim at the highest level of compatibility, meaning:

* The EVM used in Polyjuice shall be almost 100% compatible with the latest fork version of Ethereum;
* Via a [Web3 layer](https://github.com/nervosnetwork/godwoken-web3), Polyjuice shall be 100% compatible with Ethereum respecting to Web3 interfaces;

However, due to drastically different architecture and design considerations, there will inevitably be some differences between Polyjuice and Ethereum. This article aims to document and communicate such caveats.

## Account Creation

One must create an account on a Godwoken chain in order to use Polyjuice on that Godwoken chain.

There are two ways to create a layer2 account:

1. Deposit funds to Godwoken at layer1.
2. Calling Godwoken builtin [meta_contract](https://github.com/nervosnetwork/godwoken-scripts/blob/86b299f/c/contracts/meta_contract.c) to create an account at layer2.

## pCKB

**pCKB** is a fixed layer2 sUDT token type chosen when deploying a Polyjuice chain. **pCKB** to a Polyjuice chain is analogous to ETH to an Ethereum chain: it is used for charging transaction fees. The gas price of Polyjuice transactions is measured using **pCKB** designated for the Polyjuice chain, which will be deducted from the sender's account when the transaction is committed on chain.

By default a Polyjuice chain use CKB as **pCKB**. While different Polyjuice chains might use different token type as **pCKB**.

## All Tokens Are ERC20 Tokens

Ethereum differs in the processing of ERC20 tokens, and native ETH tokens. This is also the reason why wETH is invented. Godwoken conceals this difference:

Whether you use a native CKB or any sUDT token type, they will all be represented in Godwoken as a layer2 sUDT type. Polyjuice starts from this layer2 sUDT [contract](https://github.com/nervosnetwork/godwoken-polyjuice/blob/b9c3ad4/solidity/erc20/SudtERC20Proxy_UserDefinedDecimals.sol) and ensures that all the tokens on Godwoken are in compliance with the ERC20 standard, no matter if they are backed by a native CKB or a sUDT. This means you don't need to distinguish between native token and ERC20 tokens. All you have to deal with is the same ERC20 interface for all different tokens.

