# Finality Mechanism Changes

https://github.com/godwokenrises/godwoken/pull/836 introduces a new finality mechanism.

The following guide is for developers whose program needs to determine the layer-2 finality, such as an assets bridge or an off-chain program reads finalized state from layer-2. You can skip this document if you are a layer-2 EVM contract developer and do not care about the finality.

## What Has Changed

### `Timepoint`

To understand specific changes, we must understand `Timepoint`, a new underlying type introduced in https://github.com/godwokenrises/godwoken/pull/836.
A `Timepoint` is a type, underlying `u64`, that is interpreted according to its highest bit.

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

> **NOTE**: **[`GlobalState.last_finalized_block_number`](https://github.com/godwokenrises/godwoken/blob/5617b579927d85509e8f88ac4fb4493ef449b642/crates/types/schemas/godwoken.mol#L33) was renamed to [`GlobalState.last_finalized_timepoint`](https://github.com/godwokenrises/godwoken/blob/f71d2bf86f8da8873522b3655de0b4d4866ac965/gwos/crates/types/schemas/godwoken.mol#L34).**

`GlobalState.last_finalized_timepoint` is changed to type `Timepoint`:
- The `timepoint_flag == 0` indicates the `timepoint_value` represents the **finalized block number**, so any blocks with a lower number are finalized.
- The `timepoint_flag == 1` indicates the `timepoint_value` represents the **finalized timestamp**, so any blocks with a lower timestamp are finalized.

### Interpretation of `WithdrawalLockArgs.withdrawal_finalized_timepoint`

> **NOTE**: **[`WithdrawalLockArgs.withdrawal_block_number`](https://github.com/godwokenrises/godwoken/blob/5617b579927d85509e8f88ac4fb4493ef449b642/crates/types/schemas/godwoken.mol#L206) was renamed to [`WithdrawalLockArgs.withdrawal_finalized_timepoint`](https://github.com/godwokenrises/godwoken/blob/f71d2bf86f8da8873522b3655de0b4d4866ac965/gwos/crates/types/schemas/godwoken.mol#L209).**

`WithdrawalLockArgs.withdrawal_finalized_timepoint` was changed to type `Timepoint`:
- If `timepoint_flag == 0` then the `timepoint_value` represents the **withdrawn block number**, so it is finalized when the tip block number exceeds `rollup_config.finality_blocks` blocks higher than the **withdrawn block number**.
- If `timepoint_flag == 1` then the `timepoint_value` represents the **withdrawn finalized timestamp**, so it is finalized when `GlobalState.last_finalized_timepoint` exceeds the **withdrawn finalized timestamp**.

## Finality determination changes

Developers must upgrade their code to adapt to the finality changes before Godwoken activates the feature. The following code can handle the finality determination correctly before and after the new finality mechanism.

### Determine finalized blocks

```rust
const TIMEPOINT_FLAG_MASK: u64 = 1 << 63;

fn is_block_finalized(global_state: &GlobalState, l2block: &L2Block) -> bool {
    match Timepoint::from_full_value(global_state.last_finalized_timepoint().unpack()) {
        Timepoint::BlockNumber(bn) => bn >= l2block.raw().number().unpack(),
        Timepoint::Timestamp(ts)   => ts >= l2block.raw().timestamp().unpack(),
    }
}
```

### Determine finalized withdrawals

```rust
const TIMEPOINT_FLAG_MASK: u64 = 1 << 63;

fn is_withdrawal_finalized(
    rollup_config: &RollupConfig,
    global_state: &GlobalState,
    withdrawal_lock_args: &WithdrawalLockArgs
) -> bool {
    let withdrawn_timepoint = withdrawal_lock_args.withdrawal_finalized_timepoint().unpack();
    let finalized_timepoint = global_state.last_finalized_timepoint().unpack();
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

### Estimate the future pending time of withdrawals

```rust
const TIMEPOINT_FLAG_MASK: u64 = 1 << 63;
const ESTIMATE_BLOCK_INTERVAL: u64 = 36000; // in milliseconds

fn estimate_future_pending_time(
    rollup_config: &RollupConfig,
    global_state: &GlobalState,
    withdrawal_lock_args: &WithdrawalLockArgs,
) -> u64 {
    let finalized_block_number = global_state.block().count().unpack() - 1 - rollup_config.finality_blocks().unpack();
    match Timepoint::from_full_value(withdrawal_lock_args.withdrawal_finalized_timepoint().unpack()) {
        Timepoint::BlockNumber(wbn) => {
            max(0, wbn - finalized_block_number) * ESTIMATE_BLOCK_INTERVAL
        }
        Timepoint::Timestamp(wts) => {
            assert!(
                global_state.last_finalized_timepoint().unpack() >= TIMEPOINT_FLAG_MASK,
                "Observing a timestamp-based withdrawal, we can be sure that global_state.last_finalized_timepoint() is also timestamp-based"
            );
            let finalized_timestamp = global_state.last_finalized_timepoint().unpack() ^ TIMEPOINT_FLAG_MASK;
            max(0, wts - finalized_timestamp)
        }
    }
}
```

## RPC Changes

### TestMode `tests_get_global_state`

After activate the feature, the response of the RPC will changed, the field:

``` json
    "last_finalized_block_number"
```

will changed to 

``` json
    "last_finalized_timestamp"
```
