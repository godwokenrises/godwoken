# Optimistic rollup

## Architecture

Godwoken composited by the following parts:

### On-chain

* State validator - a type script maintains the global state of all accounts and all layer2 blocks.
* Challenge - a type script that handles challenge requests.

### Off-chain

* Aggregator - an off-chain program that collects layer2 transactions and submits layer2 blocks to the State validator regularly.
* Validator - an off-chain program that continuously watches the two contracts. The validator sends challenge request to contracts when a invalid states is submitted.

Usually, an aggregator is also a validator.

## Layer2 structures

### Account

An account is constructed by fields `(id: u32, nonce: u32, pubkey: Pubkey, code_hash: Byte32Opt)`:

- id, the unique id of the account
- nonce, an increment nonce
- pubkey, account's pubkey
- code_hash, blake2b hash of the contract code, for an EOA(external owned account) the code_hash is none

States of accounts are all accumulated in the global merkle tree, so we don't have a per account `state_root`.

> Currently, the account's pubkey is a secp256k1 pubkey hash; we are planning to migrate it to BLS in the later phase.

## Layer2 assets representation

To participant in the nervos network economy, we should be able to transfer assets between layer1 and layer2.

A basic idea is to write an equivalent contract on layer2 to receive layer1 assets.

So we designed a Simple UDT equivalent contract on layer2.

Due to the fact this contract is used to receive layer1 assets, the contract is designed to supports all kinds of SUDT.

We use the `type_script_hash` of SUDT cells as token_id to distinguish different tokens, which described in the RFC:
https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0025-simple-udt/0025-simple-udt.md#sudt-cell

We provide basic APIs to support query balance and transfer token:

* query(token_id, to) -> value
* transfer(token_id, to, value)

A layer2 user is supposed to use these two API to query and transfer token. For simplicity, we also treat the CKB as a SUDT token which token_id is all zero: `0x0000...0000`.

### Mint & Burn

When a new user deposits assets to join to layer2, the layer2 SUDT contract mint new tokens to the user's account.
When a user withdraws assets and leaves layer2, the SUDT contract burn equivalent tokens from the user's account.

Aggregators operate Mint & Burn by directly modify the state tree, and the state-validator contract ensure the state is valid.

## Builtin accounts

There are several builtin accounts in the Godwoken:

* `RESERVED_ACCOUNT: 0` - the zero id account is reserved for special use, for example, tansfer to 0 to burn tokens or send a transaction to 0 to create new contracts.
* `SUDT_ACCOUNT: 1` - the simple UDT contract.

## State validator

Godwoken contract supports several actions to update the global state:

* create account
* deposit
* submit block
* revert block
* prepare_withdraw
* withdraw

## Join & Leave

### Join rollup (Deposit)

To join a layer2 Rollup, users need to create deposition request cells on-chain.

``` sh
Cell {
  lock: Script {
    code_hash: <deposition_lock>
    hash_type: <...>
    args: <rollup_code_hash|pubkey_hash>
  }
  capacity: <capacity>
  data: <empty or valid UDT data>
  type_: <none or an valid UDT type>
}
```

Users put CKB or UDTs into deposition request cells, then wait for aggregators to collect them.

The lock script `deposition_lock` allows two unlock conditions:

1. The owner unlocks this cell with a recoverable secp256k1 signature; the lock script compares the recovered `pubkey_hash` with the args and returns success if `pubkey_hash` is matched.
2. An off-chain aggregator unlocks this cell in the same transaction that updates the Rollup's global state; the lock script checks there exist an input cell matches `rollup_code_hash` and return success.

After the aggregator collects the cells, the states of cells will be accumulated into the global state, and ownership of cells will be transferred to the `state validator` contract.

### Leave rollup (Withdraw)

To withdraw assets back to layer1, users firstly send a withdrawal request to the aggregator, the aggregator moves assets into a withdrawal queue and burns the assets from layer2, then users need to wait for a timeout, finally, the aggregator releases assets on layer1.

Suppose the aggregator refuses to move assets into a withdrawal queue or refuses to withdraw assets to layer1 (censorship). A user should call force-withdraw on the `state validator` contract to complete the withdrawal.

> The timeout parameter C defines an upper bound of the challenge period; after the C timeout, if we still can't prevent a malicious user from withdrawing assets to layer1, the rollup system should be considered as corrupt.

## Layer2 assets representation

Since our Rollup is based on the account model, we want to use a natural way to represent assets in layer2 account: all layer1 assets represented as states in layer2 accounts.

For example, the layer1 CKB is represented as a key-value record in the layer2 CKB token account (`account_id -> amount`). It is the same for other UDT assets; they are stored in different layer2 UDT accounts.

We also maintain a layer1 to layer2 contract map to keep consensus of assets between layers, we use [sparse merkle tree] to represent the contract map, and put the merkle root into the global state. For easy to understand, we can consider the map is fixed, which means we can only accept limited UDTs; however, it is trivial to design a mechanism to dynamically updating the map.

## Challenge

Usually, to prove a state is invalid, the challenger needs to collect enough information and post this information to the on-chain dispute contract, then the disputed contract executes the layer2 contract in a VM; if the VM exit with exceptions or exit with a different state we know that the original state is invalid.

However, this approach requires we implement a VM (layer2) in VM (CKB-VM) mechanism, if we deploy a CKB-VM upon CKB-VM, the costs of cycles will be extremely high (I haven't tested it, but I doubt if we can do a secp256k1 verification within the block limitation), if we choose to deploy other light-weight VMs like duktape or EVM, then we can't gain the benefits from CKB-VM community, we can't use the cryptography primitives that provided for CKB-VM.

So here we propose a new challenge mechanism, the challenge process is managed by the challenge contract:

* Challenger creates a challenge request cell, the type script is set to challenge contract; the args field contains `script_hash` of the validator contract, `block_hash` and `index` of the target transaction; The challenge request also requires to deposit a small amount coins.
* If validator found a challenge request, it will run it locally, if the request is an incorrectly(the challenge target state is valid), the challenge will prepare a context cell to cancel the request and take the deposited coins as a reward.
  * This is the critical step to avoid VM-in-VM. The validator uses an extra context cell to load the verification context.
  * The challenge contract must carefully verify the context is correct according to the challenge target.
  * The layer2 contract actually is also a layer1 contract, it loads the context and does the verification; if verification is failed, the whole tx will also be failed.
* After time T, if the challenge request still exists, we assume the challenge is correct.
* A validator or the original challenger can use the challenge request cell as proof to revert a layer2 block.

Compare to the 'traditional' challenge process, we require a more strict online time for validators. If a validator takes more than T time offline(or the validator can't cancel an invalid challenge request within T times due to software bug or network issue), he may lose the coins due to a malicious challenge request. Even we allow other validators to cancel a challenge request; it is still a dangerous behavior.

In the case that the validator became malicious, our challenge mechanism requires T time to revert the block, which the traditional challenge can revert the block in almost one block time. If the challenge sends another invalid block after the revert block, we need extra T times to invalid it; this means if the aggregator costs `N * COINS_TO_BE_AGGREGATOR`, we need to wait for `N * T` times to revert the block to a correct state in the worst case.


### layer2 contracts

As we mentioned in the previous section, our layer2 contracts are just layer1 contracts which implement the layer2 contract interface. A layer2 contract needs to be run in the two environments: the aggregator context and the on-chain context.

This leads to a potential consensus split risk. Since any user can create layer2 contracts, a malicious user may create a contract that behaves differently in the two contexts, or just takes some random behaviors such as returns failure if the last bit of `tx_hash` is 0, otherwise return success. This kind of contract is dangerous; when an aggregator execute the contract offchain it returns the result A, and then when a challenge request is created, the on-chain validator executes the contract and returns another result B, this kind of inconsistency causes the aggregator failed to cancel the challenge and lose the money!

To keep the contract behavior consistency, We must restrict the contract to only access the consistent environment (includes verification context, VM registers, and VM memories); any difference in the environment may lead to different contract behaviors under the two contexts.

We have several options to restrict the layer2 contract:

* Only allows contracts which in the trust list. (A semi-trusted solution)
* Run contract in a VM. (EVM, or WASM VM or JS VM)
* Create a sandbox environment for layer2 contract. (Using static check to restrict the contract)

We will implement the trust list solution in the first version and support VM and Sandbox in the future.

![Cancel a challenge request](./cancel_a_challenge_request.jpg)

[sparse merkle tree]: https://github.com/jjyr/sparse-merkle-tree "sparse merkle tree"

