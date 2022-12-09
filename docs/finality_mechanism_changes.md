# Finality Mechanism Changes - from the perspective of dApp developers

https://github.com/godwokenrises/godwoken/pull/836 changes the way to determine finality. The following guide is for dApp developers to help them understand what has changed and how to adapt.

## What Has Changed

### `Timepoint`

To understand specific changes, we must understand `Timepoint`, a new underlying type introduced in https://github.com/godwokenrises/godwoken/pull/836.
A `Timepoint` is a type, underlying `u64`, that is interpretated according to its highest bit.

  - When the highest bit is `0`, the rest bits are represented by block number
  - When the highest bit is `1`, the rest bits are represented by timestamp

For convenience, we name its highest bit `timepoint_flag`, and the rest bits `timepoint_value`.

For a deeper understanding, you can read the [Rust implementation](https://github.com/godwokenrises/godwoken/blob/bcc68b480acb292b625141a2ab7d2e6b69575f55/crates/types/src/core.rs#L292-L329).

Here are some test vectors:

| timepoint full value  | timepoint full value in binary                               | Interpretation             |
| --------------------- | ------------------------------------------------------------ | -------------------------- |
| `0`                   | `b0000000000000000000000000000000000000000000000000000000000000000` | `BlockNumber(0)`           |
| `7489999`             | `b0000000000000000000000000000000000000000011100100100100111001111` | `BlockNumber(7489999)`     |
| `9223372036862265807` | `b1000000000000000000000000000000000000000011100100100100111001111` | `Timestamp(7489999)` |
| `9223372036854775808` | `b1000000000000000000000000000000000000000000000000000000000000000` | `Timestamp(0)`        |

### Interpretation of `GlobalState.last_finalized_timepoint`

> **NOTE**: **[`GlobalState.last_finalized_block_number`](https://github.com/godwokenrises/godwoken/blob/5617b579927d85509e8f88ac4fb4493ef449b642/crates/types/schemas/godwoken.mol#L33) was renamed to [`GlobalState.last_finalized_timepoint`](https://github.com/godwokenrises/godwoken/pull/891/files#diff-96e540dc83a433d447e1d2dae392fc5eafce72e839ea3900f6f1f8638aaada6bL34-R34).**

`GlobalState.last_finalized_timepoint` was changed to type `Timepoint`:
- The `timepoint_flag == 0` indicates that its `timepoint_value` is the **finalized block number**, so any blocks with a lower number are finalized.
- The `timepoint_flag == 1` indicates that its `timepoint_value` is the **finalized timestamp**, so any blocks with a lower timestamp are finalized.

### Interpretation of `WithdrawalLockArgs.finalized_timepoint`

> **NOTE**: **[`WithdrawalLockArgs.withdrawal_block_number`](https://github.com/godwokenrises/godwoken/blob/5617b579927d85509e8f88ac4fb4493ef449b642/crates/types/schemas/godwoken.mol#L206) was renamed to [`WithdrawalLockArgs.finalized_timepoint`](https://github.com/godwokenrises/godwoken/pull/836/files#diff-96e540dc83a433d447e1d2dae392fc5eafce72e839ea3900f6f1f8638aaada6bL206-R209).**

`WithdrawalLockArgs.finalized_timepoint` was changed to type `Timepoint`:
- If `timepoint_flag == 0` then its `timepoint_value` is the **withdrawn block number**, so it becomes finalized when the tip block number exceeds `rollup_config.finality_blocks` blocks above the **withdrawn block number**.
- If `timepoint_flag == 1` then its `timepoint_value` is the **withdrawn block timestamp**, so it becomes finalized when `GlobalState.last_finalized_timepoint` exceeds the **withdrawn block timestamp**.

## How to Adapt

### For dApps that determine and filter finalized blocks

```rust
const TIMEPOINT_FLAG_MASK: u64 = 1 << 63;

fn is_block_finalized(global_state: &GlobalState, l2block: &L2Block) -> bool {
    match Timepoint::from_full_value(global_state.last_finalized_timepoint().unpack()) {
        Timepoint::BlockNumber(bn) => bn >= l2block.raw().number().unpack(),
        Timepoint::Timestamp(ts)   => ts >= l2block.raw().timestamp().unpack(),
    }
}
```

### For dApps that determine and filter finalized withdrawals

```rust
const TIMEPOINT_FLAG_MASK: u64 = 1 << 63;

fn is_withdrawal_finalized(
    rollup_config: &RollupConfig,
    global_state: &GlobalState,
    withdrawal_lock_args: &WithdrawalLockArgs
) -> bool {
    let withdrawn_finalized_timepoint = withdrawal_lock_args.finalized_timepoint().unpack();
    let finalized_timepoint = global_state.tip_block_timestamp().unpack();
    let finalized_block_number = global_state.block().count().unpack() - 1 - rollup_config.finality_blocks().unpack();

    // Alternatively, you can use a shorter equivalent code snippet:
    //
    // ```rust
    // (withdrawn_timepoint <= finalized_block_number) ||
    //     (withdrawn_timepoint ^ TIMEPOINT_FLAG_MASK) <= (finalized_timepoint ^ TIMEPOINT_FLAG_MASK)
    // ```
    //
    // Or you may want to translate it into SQL query:
    //
    // ```sql
    // SELECT *
    // FROM withdrawal
    // WHERE (timepoint <= $finalized_block_number) OR
    //       (timepoint XOR $timepoint_flag_mask) <= ($finalized_timepoint XOR timepoint_flag_mask);
    // ```
    //
    match Timepoint::from_full_value(withdrawn_timepoint) {
        Timepoint::BlockNumber(wbn) => wbn <= finalized_block_number,
        Timepoint::Timestamp(wts) => wts < (finalized_timepoint ^ TIMEPOINT_FLAG_MASK)
    }
}
```

### For dApps that estimate the future pending time of withdrawals

```rust
const TIMEPOINT_FLAG_MASK: u64 = 1 << 63;
const ESTIMATE_BLOCK_INTERVAL: u64 = 36000; // in milliseconds

fn estimate_future_pending_time(
    rollup_config: &RollupConfig,
    global_state: &GlobalState,
    withdrawal_lock_args: &WithdrawalLockArgs,
) -> u64 {
    let finalized_block_number = global_state.block().count().unpack() - 1 - rollup_config.finality_blocks().unpack();
    match Timepoint::from_full_value(withdrawal_lock_args.finalized_timepoint().unpack()) {
        Timepoint::BlockNumber(wbn) => {
            max(0, wbn - finalized_block_number) * ESTIMATE_BLOCK_INTERVAL
        }
        Timepoint::Timestamp(wts) => {
            assert!(
                global_state.last_finalized_timepoint().unpack() >= TIMEPOINT_FLAG_MASK,
                "Observing a timestamp-based withdrawal, we can be sure that global_state.last_finalized_timepoint() is also timestamp-based"
            );
            let finalized_timestamp = global_state.tip_block_timestamp().unpack();
            max(0, wts - finalized_timestamp)
        }
    }
}
```
