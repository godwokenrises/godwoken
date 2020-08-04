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

(TODO)

## Contracts

### State validator

Godwoken contract supports several actions to update the global state:

* create account
* deposit
* submit block
* revert block
* prepare_withdraw
* withdraw

`create_account`, deposit layer1 assets to layer2, and create a new account. The `index` of the new account must be `last_account.index + 1`; the `nonce` must be `0`; `script` can be set to `None` or a contract.

`deposit`, deposit layer1 assets layer2 and update the `account_root`.

`submit block`, only accounts with required balance can invoke this action. The caller needs to commit `block`, `transactions`, and merkle proof; `transactions` doesn't do verification on-chain; when invalid state is committed, other users can send a challenge request to penalize the commiter and revert the state.

`revert block`, the challenge logic is handling by challenge contract, here we only care about the result of the challenge request. Anyone has an account can send a `revert block` request with a challenge result cell. If the challenge result is valid, the invalid block will be replaced with a revert block: `Block { (untouched fields: number, previous_account_root), tx_root: 0x00..00, ag_sig: 0x00..00, ag_index: challenger_account_index, account_root: new_account_root, invalid_block: Some(0x...block_hash) }`, in the `new_account_root` state, a part of the invalid block's aggregator's CKB is sent to challenger's account as the reward.

`prepare_withdraw`, move assets to a withdrawing queue.

`withdraw`, move assets from withdrawing queue to layer1, a withdrawable assets in the queue must wait for at least `WITHDRAW_WAIT` blockssince enqueued. 


### Challenge

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

![Cancel a challenge request](./cancel_a_challenge_request.jpg)

Compare to the 'traditional' challenge process, we require a more strict online time for validators. If a validator takes more than T time offline(or the validator can't cancel an invalid challenge request within T times due to software bug or network issue), he may lose the coins due to a malicious challenge request. Even we allow other validators to cancel a challenge request; it is still a dangerous behavior.

In the case that the validator became malicious, our challenge mechanism requires T time to revert the block, which the traditional challenge can revert the block in almost one block time. If the challenge sends another invalid block after the revert block, we need extra T times to invalid it; this means if the aggregator costs `N * COINS_TO_BE_AGGREGATOR`, we need to wait for `N * T` times to revert the block to a correct state in the worst case.

[sparse merkle tree]: https://github.com/jjyr/sparse-merkle-tree "sparse merkle tree"

