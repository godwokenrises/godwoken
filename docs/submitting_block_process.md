# Submitting Block Process

The Submitting Block Process refers to the process by Godwoken block producers submitting a new block and global state of L2 for the chain to be updated. It is essentially a state transition. Ensuring the validity of the state transition process is one of the core responsibilities of the [_State Validator_ script](https://github.com/godwokenrises/godwoken/blob/df96ea55a87bdec118b3457173792bb4d78aae8a/gwos/contracts/state-validator/src/verifications/submit_block.rs).

First, let's examine the general process of the submit block, then analyze the meaning of the various data involved and how validity is checked.

---

There exists a live cell, named _rollup_state_cell_, holds L2's global state, which is encoded in [`GlobalState`](https://github.com/godwokenrises/godwoken/blob/9a568d89a88344797289f5e18cf082078e246c85/crates/types/schemas/godwoken.mol#L26-L38). L2 block producer will send a L1 transaction, named _submitting_block_transaction_, to consume the live _rollup_state_cell_ and output the new _rollup_state_cell_ which holds the next global state. To enforce the transition process, or say, to enforce the validity of the updated global state, the next L2 block will be carried in _submitting_block_transaction_'s [`witness`](https://github.com/nervosnetwork/rfcs/blob/e4e53deb21f6ce58708cfcdead69526ea9355952/rfcs/0019-data-structures/0019-data-structures.md#description-2), so that the State Validator script can verify the carried L2 block and then deduce the correct global state from the verified L2 block.

For convenience, we call the carried L2 block _l2block_, the consumed _rollup_state_cell_'s global state _prev_global_state_, and the output _rollup_state_cell_'s global state _post_global_state_.

the State Validator script uses _prev_global_state_ as certain input to enforce the validity of _l2block_ and then enforce _post_global_state_, or say, enforce the submitting block process.

## Enforce _l2block_
In this section, I'll give the data structure of _l2block_, sourced from [`L2Block`](https://github.com/godwokenrises/godwoken/blob/6148a733fcf7b41ce25aee3a44e8f2ae6158390d/crates/types/schemas/godwoken.mol#L115-L122), and explain what each piece of information means and how we can ensure its validity.

```
table L2Block {
    kv_state: vector KVPairVec <struct KVPair { k: Byte32, v: Byte32 }>,
    kv_state_proof: Bytes,
    block_proof: Bytes,
    transactions: L2TransactionVec,
    withdrawals: WithdrawalRequestVec,
    raw: struct RawL2Block {
        number: Uint64,
        block_producer: Bytes,
        parent_block_hash: Byte32,
        stake_cell_owner_lock_hash: Byte32,
        timestamp: Uint64,
        prev_account: struct AccountMerkleState {
            merkle_root: Byte32,
            count: Uint32,
        },
        post_account: struct AccountMerkleState {
            merkle_root: Byte32,
            count: Uint32,
        },
        state_checkpoint_list: Byte32Vec,
        submit_withdrawals: struct SubmitWithdrawals {
            withdrawal_witness_root: Byte32,
            withdrawal_count: Uint32,
        },
        submit_transactions: struct SubmitTransactions {
            tx_witness_root: Byte32,
            tx_count: Uint32,
            prev_state_checkpoint: Byte32,
        }
    }
}
```

- `l2block.kv_state` and `L2Block.kv_state_proof`
  Continue to the chapter "Apply Deposits and Withdrawals.".
  
- `l2block.block_proof`
  This is used for proving that the merkle tree for `post_global_state.block.merkle_root` contains the present l2block while `prev_global_state.block.merkle_root` does not.
  Data like this does not need to be enforced since it is proof data.

- `l2block.transactions`
  This is L2 transations original data.

  **The challenge mechanism in Optimistic Rollup ensures that l2block.transactions are correct.**

- `l2block.withdrawals`
  This is L2 withdrawals original data.

  _The State Validator_ script must ensure there exists withdrawal cells in _submitting_block_transaction_'s outputs match the L2 withdrawals.

- `l2block.raw.number`
  Assert that `l2block.raw.number` must be equal to [`prev_global_state.block.count`](https://github.com/nervosnetwork/godwoken/blob/6148a733fcf7b41ce25aee3a44e8f2ae6158390d/crates/types/schemas/godwoken.mol#L29).

- `l2block.raw.block_producer`
  Skip due to out of topic of this article.

- `l2block.raw.parent_block_hash`
  Assert that `l2block.raw.parent_block_hash` must be equal [`prev_global_state.tip_block_hash`](https://github.com/nervosnetwork/godwoken/blob/6148a733fcf7b41ce25aee3a44e8f2ae6158390d/crates/types/schemas/godwoken.mol#L31).

- `l2block.raw.stake_cell_owner_lock_hash`
  Skip due to out of topic of this article.

- `l2block.raw.timestamp`
  Assert that `l2block.raw.timestamp` must be greater than [`prev_global_state.tip_block_timestamp`](https://github.com/nervosnetwork/godwoken/blob/6148a733fcf7b41ce25aee3a44e8f2ae6158390d/crates/types/schemas/godwoken.mol#L32), in order to enforce the increase of block timestamps; and less than `_submitting_block_transaction_.since`, in order to prevent the block timestamp from exceeding actual time.

- `l2block.raw.prev_account`
  Assert that `l2block.raw.prev_account` must be equal to [`prev_global_state.account`](https://github.com/nervosnetwork/godwoken/blob/6148a733fcf7b41ce25aee3a44e8f2ae6158390d/crates/types/schemas/godwoken.mol#L28).

- `l2block.raw.post_account`
  `l2block.raw.post_account` represents the global accounts tree, also called the World State. As defined in the blockchain protocol, the previous World State will transit to the new World State by applying deposits/withdrawals/L2Transactions included in the present _l2block._

  _State Validator_ is able to apply the deposits and withdrawals(read more from chapter "Apply Deposits and Withdrawals"). However, as an Optimistic Rollup framework, Godwoken does not execute l2transactions on L1. This means the L1 scripts are not able to actually verify the _World State_ by directly applying l2transactions. Optimistic Rollup's challenge mechanism takes the responsibility to enforce the correctness of `L2Block.raw.post_account`.

- `l2block.raw.state_checkpoint_list`
  `l2block.raw.state_checkpoint_list` contains a list of checkpoints that represents the _World State_ after applying every transactions. Therefore, its length must be equal to `l2block.transactions`.

   Besides, Optimistic Rollup's challenge mechanism takes the responsibility to enforce the correctness of `l2block.raw.state_checkpoint_list`.

- `l2block.raw.submit_withdrawals`
  ❓

- `l2block.raw.submit_transactions`
  ❓

## Enforce _post_global_state_
It's able to deduce _post_global_state_ from _prev_global_state_ and the verified _l2block_.

```
struct GlobalState {
    rollup_config_hash: Byte32,
    account: struct AccountMerkleState {
	    merkle_root: Byte32,
	    count: Uint32,
	},
    block: struct BlockMerkleState {
	    merkle_root: Byte32,
	    count: Uint64,
	},
    reverted_block_root: Byte32,
    tip_block_hash: Byte32,
    tip_block_timestamp: Uint64,
    last_finalized_block_number: Uint64,
    status: byte,
    version: byte,
}
```

- `post_global_state.rollup_config_hash`
  This is used for identifying [rollup config](https://github.com/nervosnetwork/godwoken/blob/6148a733fcf7b41ce25aee3a44e8f2ae6158390d/crates/types/schemas/godwoken.mol#L52) and not allowed to change.
- `post_global_state.account`
  Assert that it is equal to `l2block.post_account`.
- `post_global_state.block`
  This represents the merkle tree of l2blocks. This can be deduced by `prev_global_state.block` and `l2block.block_proof`.
- `post_global_state.reverted_block_root`
  Skip due to out of topic of this article.
- `post_global_state.tip_block_hash`
  Assert that it is equal to `l2block.hash()`.
- `post_global_state.tip_block_timestamp`
  Assert that it is equal to `l2block.timestamp`.
- `post_global_state.last_finalized_block_number`
- `post_global_state.status`
  This represents the chain status, halted or not. This is controlled by block producer.
- `post_global_state.version`
  This presents the consensus version of L2 chain. This is controlled by block producer. 

## Apply Deposits and Withdrawals
Deposits and withdrawals will change the target accounts balance. For example, deposit 10 CKB means that the CKB balance of L2 account will increase by 10. To enforce that the block producer has properly processed deposits/withdrawals, adding or subtracting the balance of the corresponding accounts, and to get the _World State_ afterwards correctly (will be used as the _World State_ before processing l2transactions), Godwake does this:
1. `l2block.kv_state` contains the target accounts balance in the parent _World State_. `l2block.kv_state_proof` proves `l2block.kv_state` truly exist in the parent _World State_.
2. _State Validator_ adds or substracts target accounts balances, initialized by `l2block.kv_state`, according to the deposits and withdrawals. Finally, we got a correct _new_kv_state_.
3. Calculated the _World State_ by `new_kv_state.calculate_state_checkpoint()`
