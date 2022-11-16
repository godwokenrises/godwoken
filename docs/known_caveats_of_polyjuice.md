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

## Additional limitations

To prevent the DOS and make it easy to perform a challenge on-chain, Godwoken runtime set some additional limitations when verifying a tx:

1. Max transaction size - Max tx size is 50KB, the size is calculated in Godwoken tx format, so it is slightly different from the Ethereum RLP encoded format.
2. Max withdrawal size - Max withdrawal size is 50KB.
3. Max transaction cycles - Max tx cycles is 500M, and Godwoken runs EVM in the CKB-VM. Besides the gas limit, we also set a cycle limit on the CKB-VM.
4. Max write data - Max write data limit the data size per write to 25 KB.
5. Max total read data - 2M, total read data in a single transaction.
6. Max return data - the return data of a transaction is limited to 128KB

These restrictions aim to prevent DOS attacks, and normal transactions shouldn't be affected. If you are running a normal transaction call and get into trouble with these limitations, please open an issue to the project.
