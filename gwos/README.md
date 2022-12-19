# Godwoken-scripts

On-chain scripts of [Godwoken](https://github.com/nervosnetwork/godwoken) project.

## Directory structure

``` txt
root
├─ c: Layer-2 built-in C scripts
│  ├─ contracts/meta_contract.c: The Meta contract operating layer-2 accounts
│  ├─ contracts/eth_addr_reg.c: Mapping Ethereum address to Godwoken account
│  ├─ contracts/sudt.c: The layer-2 Simple UDT contract
│  ├─ contracts/examples: Example contracts
├─ c-uint256-tests: tests of uint256 C implementation
├─ contracts: Layer-1 Godwoken scripts
│  ├─ always-success: A script always returns true, used in tests
│  ├─ challenge-lock: The lock script checks setup of a challenge
│  ├─ ckb-smt: SMT no-std implementation
│  ├─ custodian-lock: The lock script protects custodian cells
│  ├─ deposit-lock: The lock script of user deposits
│  ├─ eth-account-lock: The lock script used to check Ethereum signatures on-chain
│  ├─ gw-state: Godwoken state tree implementation
│  ├─ gw-utils: Common functions used in Godwoken scripts
│  ├─ secp256k1-utils: Secp256k1
│  ├─ stake-lock: The lock script of stake cell
│  ├─ state-validator: The type script constaint the on-chain operation of Rollup cell
│  ├─ withdrawal-lock: The lock script protects withdrawal cells
├─ tests: scripting tests
├─ tools: tools used in CI
```

## Scripts

Godwoken scripts are written in Rust and C, Rust scripts are running upon CKB to constrain the Rollup behavior, and C scripts are running in Godwoken's node to provide layer-2 built-in contracts and programming interface.

The Rust scripts are located in the `contracts` directory, using the command `capsule build` to build.
The C scripts are located in the `c` directory, using the command `cd c && make` to build.

All data structures are using [molecule](https://github.com/nervosnetwork/molecule) format to do the serialization, which is defined in the [godwoken.mol](https://github.com/nervosnetwork/godwoken/blob/develop/crates/types/schemas/godwoken.mol) file. 

Overview introduction of Godwoken mechanism: [Life of a godwoken transaction](https://github.com/nervosnetwork/godwoken/blob/develop/docs/life_of_a_godwoken_transaction.md) and [Life of a polyjuice transaction](https://github.com/nervosnetwork/godwoken/blob/develop/docs/life_of_a_polyjuice_transaction.md)

### State validator

State validator is the major script to verify the on-chain Rollup cell.
Rollup cell is an identity cell on CKB, it stores the structure [GlobalState](https://github.com/nervosnetwork/godwoken/blob/develop/crates/types/schemas/godwoken.mol) which represents the layer-2 state.

```
Rollup cell:
data: GlobalState,
capacity: <capacity>
lock: <lock script>,
type: <state validator script>,
```

To update the Rollup cell, we build a tx to consume the old one and generate a new Rollup cell as the tx's output.
We also need to provide a args in the tx's witness: RollupAction, which is a structure that contains layer-2 block and Merkle proof.
The state validator will make sure the state transition of the Rollup cell is valid by verifying these proofs.

The behaviors of Rollup are defined as enumerated type `RollupAction`:
- `RollupSubmitBlock`, submit a layer-2 block
  - The layer-2 transactions, deposits, and withdrawals are included in a layer-2 block structure. We won't verify txs' and withdrawals' signatures on-chain since we are using the optimistic mechanism.
  - Deposits cells are collected as inputs, and the action converts these deposit cells to custodian cells to complete the deposit.
- `RollupEnterChallenge`, A challenger submit a challenging target(transaction, withdrawal) to halt the rollup.
- `RollupCancelChallenge`, Anyone can send this action to cancel a challenge, in this action the challenge target(a tx or a withdrawal request) will actually run on the layer1 chain to prove that the challenge in the previous step is wrong. After this action, the Rollup status becomes running again.
- `RollupRevert`, if a challenge is a maturity(which means it hasn't been canceled within the challenge time). The action reverts the layer-2 block state to the parent block of the challenged block, and the stake of the block producer is penalized. We only revert the layer-2 state in this action, the reverting of layer-1 locked cells(deposit/custodian/withdrawal) are handled in the `RollupSubmitBlock` action.

There is another important structure `RollupConfig`, we defined consensus and initial Rollup settings in the cell.

The `lock` fields of the Rollup cell have relatively standalone rules, in the original design we assume everyone who stakes can submit to the Rollup, but in the initial phase, we want a more stable setup, which only the block producer can submit to the rollup.

### Stake lock

A block producer is required to provide a stake cell to perform the `RollupSubmitBlock` action.
The stake lock args is `StakeLockArgs`, after submitting a layer-2 block, the `args.stake_finalized_timepoint` is updated to the latest block's timepoint.

Stake lock can be unlocked in two paths:

1. Unlock by the submitter after `args.stake_finalized_timepoint`'s block is finalized.
2. Unlock by the challenger in the `RollupRevert` action.

### Deposit lock

A layer1 user can join the Rollup by creating a deposit cell. The Godwoken collects deposit cells from the layer1 blockchain and put them into the inputs of the tx that submit layer-2 block.

The sender can unlock a deposit cell after `cancel_timeout` if the deposit is not processed by Godwoken.

### Custodian lock

Rollup uses the custodian lock to hold the deposited assets. Custodian lock's args is a structure `CustodianLockArgs`, the field `deposit_finalized_timepoint` represents the block that the deposit is processed.

The `deposit_finalized_timepoint` also denotes whether the custodian lock is finalized or unfinalized.
For unfinalized custodian cells, once the deposit block is reverted, these cells must be also reverted to the deposit cells.
For finalized custodian cells, since they are finalized, we can free merge or split these cells.

When a withdrawal request is sent, Godwoken moves assets from finalized custodian cells to generate withdrawal cells.

### Withdrawal lock

Withdrawal cells are generated in the `RollupSubmitBlock` action according to the `block.withdrawals` field.

The withdrawal lock has two unlock paths:

1. Unlock by withdrawer after the `WithdrawalLockArgs#withdrawal_finalized_timepoint` is finalized.
2. Unlock as a reverted cell in the `RollupSubmitBlock` action, a corresponded custodian cell will be generated.

### Challenge lock

When a Godwoken node found that an invalid state exists in the Rollup, the node can send the `RollupEnterChallenge` action to the Rollup cell and generate a challenging cell.

A challenge cell must set a challenging target in its lock args `ChallengeLockArgs`. The challenging target can be a layer-2 transaction or a withdrawal request.

If the challenging cell hasn't been canceled during a maturity time, the challenger can execute the `RollupRevert` action on the Rollup cell and take stake cells which send by reverted block submitters as rewards.

If the challenge target is invalid. Other nodes can cancel this challenge by executing the `RollupCancelChallenge` action, the challenging cell must be included in the tx.inputs.
* For a withdrawal target, challenge lock verifies that an account script is in the tx.inputs to verify the signature.
* For a layer-2 transaction target, challenge lock reads the backend script code_hash from the state tree, then verifies that the backend validator script is in the tx.inputs.

## layer-2 scripts

The C scripts located in the `c` are Godwoken layer-2 scripts. A layer-2 script can be executed on CKB when a challenge happend, which means a layer-2 script is also a valid layer-1 script except it follows the special interface convenient which required by Godwoken.

Godwoken account consisted of the following fields: `(id: u32, nonce: u32, script: Script)`, the `script` fields determine which script the account used. There are two types of layer-2 scripts: lock and contract, if an account id appeared at `l2tx.from_id`, we assume the account's script is a lock which means the script follows the lock script interface convenient and it can verify signatures(like Ethereum EOA). If an account id appeared at `l2tx.to_id`, we assume the account's script is a contract which means we should execute it when a tx is sent to the account(like an Ethereum contract account).

A layer-2 contract script is run both on the on-chain and off-chain. the unified interface is defined in the `c/gw_def.h`. The on-chain implementation is `validator_utils.h`, and the off-chain implementation is `generator_utils.h`.

### ETH account lock

A layer-2 lock script.

ETH account lock is a script that verifies the layer-2 account signature.

### Meta contract

A layer-2 contract script.

A built-in layer-2 account allows creating another account by sending tx to this.

This contract args is `MetaContractArgs`, the built-in contract id is `0`.


### sUDT contract

A layer-2 contract script.

This contract keeps a consistent mapping to the layer1 sUDT, the `account.script.args` equal to a layer1 sUDT script hash. Godwoken creates a new corresponded sUDT account when a user deposits a new type of sUDT.

This contract args is `SUDTArgs`, the built-in CKB Simple UDT contract id is `1`.

### ETH address registry

A layer-2 contract handles mapping of the Ethereum address the Godwoken account.

When a user deposits token to create a new account, a corresponding Ethereum address is inserted to the contract. If the account is created through a Meta contract, the user must register the Ethereum address for the acount by calling the ETH address registry contract.

The built-in ETH address registry is allocated to id `2`.

### Polyjuice

* Repo: https://github.com/nervosnetwork/godwoken-polyjuice

The polyjuice backend for godwoken. The C scripts are located in the c directory, using the command `make all-via-docker` to build them. Using the command `bash devtools/ci/integration-test.sh` run all tests.

Polyjuice backend accepts an Ethrereum-like transaction and executes it in EVM. Here is the Ethereum [transaction structure](https://eth.wiki/json-rpc/API#eth_sendtransaction):

`(from, to, gas, gasPrice, value, data)`

In polyjuice, `from` and `to` are included in RawL2Transaction (`from_id`, `to_id`) directly. `call_kind`(CREATE/CALL), `gas`, `gasPrice`, `value` and `data` are included in `RawL2Transaction.args`.

