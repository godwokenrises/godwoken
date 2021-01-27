Life of a Polyjuice Transaction
===============================

# Overview

Polyjuice provides an [Ethereum](https://ethereum.org/en/) compatible layer on [Nervos CKB](https://github.com/nervosnetwork/ckb). It leverages account model as well scalability provided by [Godwoken](./life_of_a_godwoken_transaction.md), then integrates [evmone](https://github.com/ethereum/evmone) as an EVM engine for running Ethereum smart contracts. Polyjuice aims at 100% EVM compatibility as a goal, meaning we plan to support all smart contracts supported by the latest Ethereum hardfork version. As of the time this document is initially written, full EVM compatibility with the [Istanbul](https://eth.wiki/en/roadmap/istanbul) hardfork will be provided.

Polyjuice only provides contract accounts in Ethereum. Godwoken's user accounts are leveraged to act as EOA accounts in Ethereum.

Polyjuice is designed as 2 parts:

1. A [backend](./life_of_a_godwoken_transaction.md#backend) of Godwoken for state computation. A Polyjuice transaction is essentially just a godwoken transaction.
2. A surrounding part provides toolings for building Polyjuice transaction.

# Root Account & Deployment

Similar to the fact that multiple Godwoken deployments might be setup on one CKB blockchain, multiple different deployments of Polyjuice, can be setup on a single Godwoken blockchain. Different Polyjuice deployments here, are distinguished by different **root accounts**. Root account serves 2 purposes here:

* Different root accounts indicates different Polyjuice deployments. Different deployments might also use different settings, in the next section one example of such different settings will be explained.
* Root accounts are used to deploy smart contracts on Polyjuice.

To setup a Polyjuice root account, one sends a transaction to Godwoken's [MetaContract](./life_of_a_godwoken_transaction.md#metacontract), to create a new contract account using Polyjuice as the backend:

```
{
  "raw": {
    "from_id": "0x2",
    "to_id": "0x0",
    "nonce": "0x4",
    "args": "0x0000000041000000080000003900000010000000300000003100000020814f4f3ebaf8a297d452aa38dbf0f9cb0b2988a87cb6119c2497de817e7de9000400000001000000"
  },
  "signature": "0xc62d332f398323b972c5ee5c4481661ca9d17125af6f61e5220e2fbfe3bd325a0cc6c3ac174950dc1282d5e6059fc08838b9040ed7eca0ad13474af869f25a8701"
}
```

The `args` part in this transaction, contains a [MetaContractArgs](https://github.com/nervosnetwork/godwoken/blob/v0.1.0/crates/types/schemas/godwoken.mol#L192-L194) data structure serialized in molecule format. A JSON representation for the `args`, is as follows:

```
{
  "type": "CreateAccount",
  "value": {
      "script": {
          "code_hash": "0x20814f4f3ebaf8a297d452aa38dbf0f9cb0b2988a87cb6119c2497de817e7de9",
          "hash_type": "data",
          "args": "0x01000000"
      }
  }
}
```

The included `code_hash` and `hash_type` are pre-determined by each Godwoken deployments. The `args`(notice we have many different args here, please pay attention so you don't get confused :P) in the CreateAccount data structure, which value is `0x01000000`, contains the account ID for a layer 2 sUDT type. The specified sUDT here, will be treated as **pETH** for current Polyjuice deployments.

# pETH

**pETH** is a new concept introduced by Polyjuice. Recall that in Ethereum, the gas of each smart contract is calculated. The transaction fee is calculated then by multiplying gas with specified gas price. In Polyjuice we are preserving the same workflow for maximum compatibility, however one question arises: trasasction fee in Ethereum is calculated using ETH, what unit shall we use in Polyjuice?

To solve this problem, we introduce the concept of **pETH** in Polyjuice. In Polyjuice, **pETH** is used as the unit for calculating transaction fees. This means while the gas price is Ethereum is ETH/gas(while this is over-simplified notation, since Ethereum also has wei, but I'm sure you will get the idea), in Polyjuice gas price is measured in pETH/gas. When executing a transaction, Polyjuice will deduct transaction fees using tokens in the layer 2 sUDT type denoted by **pETH**.

Note in Ethereum, one can also send some ETH to a smart contract for certain behavior. In Polyjuice, this feature is also performed by sending pETH.

As shown above, one is free to use any Godwoken powered layer 2 sUDT type as **pETH**. This means we can use CKB, or any layer 1 sUDT as **pETH** in Polyjuice. One interesting idea, is that we can use [Force Bridge](https://github.com/nervosnetwork/force-bridge-eth) to map real ETH in Ethereum network, to a sUDT type on CKB, we then create a Polyjuice deployment using this particular sUDT type as **pETH**. The result here, is that we will have a Polyjuice deployment using **real** ETH in CKB as well.

# Actions

In this sections we will explain actions one can perform on Polyjuice, together with technical details related to each action.

## Deploy an Ethereum Smart Contract

To deploy an Ethereum Smart Contract to Polyjuice, one creates a layer 2 Polyjuice/Godwoken transaction to a Polyjuice root account. Here's the JSON representation for such a transaction:

```
{
  "raw": {
    "from_id": "0x2",
    "to_id": "0x5",
    "nonce": "0x4",
    "args": "0x0000030000000000000000000000000000000000000000000000000000000000000001900101000060806040525b607b60006000508190909055505b610018565b60db806100266000396000f3fe60806040526004361060295760003560e01c806360fe47b114602f5780636d4ce63c14605b576029565b60006000fd5b60596004803603602081101560445760006000fd5b81019080803590602001909291905050506084565b005b34801560675760006000fd5b50606e6094565b6040518082815260200191505060405180910390f35b8060006000508190909055505b50565b6000600060005054905060a2565b9056fea2646970667358221220044daf4e34adffc61c3bb9e8f40061731972d32db5b8c2bc975123da9e988c3e64736f6c63430006060033"
  },
  "signature": "0x820750bc78c68fdf9def8ee6cd7b34d49e4dc830393d82a2dd5fa882d8af16481e2795856f8445aadf2e1ddc9c040a2ee4ae097d392709f7be163a72e992ba0a01"
}
```

The `to_id` specified here, shall be a Polyjuice root account.

The `args` part uses a [custom serialization format](https://github.com/nervosnetwork/godwoken-examples/blob/2cb71f19ca4e8e2898716517c2cb940dc0747c7a/packages/polyjuice/lib/index.js#L6-L31), the JSON representation for the `args` in the above example, is as follows:

```
{
  to_id: 0,
  value: 400n,
  data: "0x60806040525b607b60006000508190909055505b610018565b60db806100266000396000f3fe60806040526004361060295760003560e01c806360fe47b114602f5780636d4ce63c14605b576029565b60006000fd5b60596004803603602081101560445760006000fd5b81019080803590602001909291905050506084565b005b34801560675760006000fd5b50606e6094565b6040518082815260200191505060405180910390f35b8060006000508190909055505b50565b6000600060005054905060a2565b9056fea2646970667358221220044daf4e34adffc61c3bb9e8f40061731972d32db5b8c2bc975123da9e988c3e64736f6c63430006060033"
}
```

* `to_id` contains the account ID for the polyjuice contract to call. In the case of deploying a smart contract, 0 is used here.
* `value` contains pETH to send to the target smart contract.
* `data` contains the actual compiled smart contract to deploy.

The returned result for running this layer 2 transaction, will contain the newly created account ID for the deployed smart contract.

## Calling a Smart Contract

A Godwoken layer 2 transaction is also used here to invoke a Polyjuice Smart Contract. Here's the JSON representation for such a layer 2 transaction:

```
{
  "raw": {
    "from_id": "0x2",
    "to_id": "0x6",
    "nonce": "0x5",
    "args": "0x000000000000000000000000000000000000000000000000000000000000000000000000040000006d4ce63c"
  },
  "signature": "0x587596568d723a178306f1440cffc09f8607cea68ed691085a743a914d9ffb9f5039667c63b2def5b8f62a12c194b9259c421d0c64468f88040cad4df11d798a00"
}
```

The `to_id` used here must be the contract account for a deployed Polyjuice Smart Contract.

The `args` part uses a [custom serialization format](https://github.com/nervosnetwork/godwoken-examples/blob/2cb71f19ca4e8e2898716517c2cb940dc0747c7a/packages/polyjuice/lib/index.js#L6-L31), the JSON representation for the `args` in the above example, is as follows:

```
{
  to_id: 6,
  value: 0n,
  data: "0x6d4ce63c"
}
```

* `to_id` in `args` is also set to the smart contract account ID to be called.
* `value` in `args` contains any pETH sent to the smart contract.
* `data` in `args` contains [Ethereum ABI compatible data payload](https://github.com/ethereumbook/ethereumbook/blob/develop/06transactions.asciidoc#transmitting-a-data-payload-to-an-eoa-or-contract) used to invoke the smart contract.

To make an analogy:

* `value` resembles Ethereum's `msg.value`
* `data` resembles Ethereum's `msg.data`
