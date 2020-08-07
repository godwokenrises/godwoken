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

## State validator

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


### Sandbox to run layer2 contracts

As we mentioned in the previous section, our layer2 contracts are just layer1 contracts in the special form. A layer2 contract needs to be run in the two environments: the aggregator context and the on-chain context.

This leads to a potential consensus split risk. Since any user can create layer2 contracts, a malicious user may create a contract that behaves differently in the two contexts, or just takes some random behaviors such as returns failure if the last bit of `tx_hash` is 0, otherwise return success. This kind of contract is dangerous; when the aggregator submits a transaction which invokes the contract, it returns a result, and then when a challenge request is created, the contract returns another result, the aggregator can't cancel the challenge and will lose the money!

To keep the contract behavior consistency, We must restrict the contract to only access the consistent environment (verification context, VM registers, and VM memories); any difference in the environment may lead to different contract behaviors under the two contexts.

To restrict the layer2 contract behavior, we need to create a sandbox for it:

Aggregator:

1. To prevent layer2 contract access inconsistent data in different environments, we must disable the syscall feature. The aggregator must scan the contract binary and reject any layer2 contract, which contains the `ecall` opcode (`ecall` is the only way to invoke syscalls).
2. After disabling the syscall, the layer2 contract can only access the verification context, which we passed to it, the verification context must be sorted in canonical order. (for example, the accounts list must sorted by ID).

Notice: The aggregator needs to run a layer2 contract at least once to generate the verification context. We use the same interface but different implementation for generator and verifier. In the generator context, the contract access data via syscalls; in the verifier context, the contract access data via reading from verification context.  This means a layer2 contract may behave differently in the generator and verifier; we must verify the transactions again after packing them into a block, and remove the transaction failed in the verification.


On-chain sandbox:

However, we still need to pass the verification context to the layer2 contract. The idea is to use a sandbox contract to setup environment for the layer2 contracts, the sandbox contract must be dedicated designed and must guarantee the verification context, VM registers, and VM memories are identical in the aggregator context and the on-chain context.

1. call `load_witness` syscall, load the verification context into the stack.
2. Do the pre merkle verification
3. Load and invoke the layer2 contract.
4. Do the post merkle verification

Using the static check to disable the syscalls, and the sandbox contract to keep a canonical environment, we can ensure the layer2 contract behavior is consistent in the aggregator context and the on-chain context.

![Cancel a challenge request](./cancel_a_challenge_request.jpg)

[sparse merkle tree]: https://github.com/jjyr/sparse-merkle-tree "sparse merkle tree"

