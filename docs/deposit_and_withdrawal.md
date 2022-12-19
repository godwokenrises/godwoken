# Deposit and withdrawal

Deposit and withdrawal is a special layer1 <-> layer2 messaging mechanism with assets transfer. Users can deposit assets from layer1 (CKB) to layer2 (godwoken), or withdraw from layer2 back to layer1.

## Deposit

A deposit request is created by sending a layer1 transaction which generates a cell with a special lock - deposit lock. The block producer will collect these cells and process the deposit requests in blocks. It will unlock the deposit cells, move the assets under the custodian lock, and update layer2 state, in block submission layer1 transactions. These transactions are checked by a layer1 script, so the block producer can't take the assets away.

The deposit cell:

``` yaml
lock:
  code_hash:    (deposit lock's code hash),
  hash_type:    Type,
  args: (rollup_type_hash(32 bytes) | DepositLockArgs)
capacity:   (deposit CKB),
type_:  (none or SUDT script)
data:   (none or SUDT amount)
```

The `lock` field of the deposited cell is using deposit lock, the first 32 bytes of `args` is a unique value associated with the rollup instance, then the data structure `DepositLockArgs` denotes which layer2 account the user deposit to. `capacity` is the total amount of CKB user deposit, the `type_` and `data` fields are following CKB Simple UDT format, with these fields users can deposit Simple UDT assets to layer2.

```
table DepositLockArgs {
    // layer1 lock hash
    owner_lock_hash: Byte32,
    layer2_lock: Script,
    cancel_timeout: Uint64,
    registry_id: Uint32,
}
```

`DepositLockArgs` denotes the layer2 account's script and `registry_id`, currently, only the ETH registry is supported. Users can cancel the deposit after `cancel_timeout`, it is used in case the block producer rejects to package the deposited cell, it happened when the deposited cell contains invalid data.

## Custodian cell

Deposit cells are converted to custodian cells when assets are deposited to layer2. Custodian cells are protected by the custodian lock, which enforces that the assets can only be transferred out when a user withdraw.

The custodian cell:

``` yaml
lock:
  code_hash:    (custodian lock's code hash),
  hash_type:    Type,
  args: (rollup_type_hash(32 bytes) | CustodianLockArgs)
capacity:   (deposit CKB),
type_:  (none or SUDT script)
data:   (none or SUDT amount)
```

The first 32 bytes of `args` is a unique value associated with the rollup instance, then the `CustodianLockArgs` records the deposit info. `capacity` is the amount of CKB, and the `type_` and `data` fields are following CKB Simple UDT format.

```
table CustodianLockArgs {
    deposit_block_hash: Byte32,
    deposit_finalized_timepoint: Uint64,
    deposit_lock_args: DepositLockArgs,
}
```

`CustodianLockArgs` saves the entire deposit info, `deposit_lock_args` is from the original deposit cell's args, `deposit_block_hash` and `deposit_finalized_timepoint` denotes the layer2 block that include the deposit.

CKB requires `capacity` to cover the cost of the cell, the `capacity` of the deposited cell must also cover the custodian cell, so the minimal deposit CKB that Godwoken allows is as follows:

* Deposit CKB: 298 CKB
* Deposit CKB and Simple UDT: 379 CKB


## Withdrawal

### Current withdrawals cell (v1)

Users must sign withdrawal requests and send them to the block producer. The block producer will process these withdrawals by updating layer2 state and convert custodian cells to withdrawal cells in block submission layer1 transactions.

The withdrawal cell:

``` yaml
lock:
  code_hash:    (withdrawal lock's code hash),
  hash_type:    Type,
  args: (rollup_type_hash(32 bytes) | WithdrawalLockArgs (n bytes) | len (4 bytes) | layer1 owner lock (n bytes))
capacity:   (CKB amount),
type_:  (none or SUDT script)
data:   (none or SUDT amount)
```

Withdrawal lock guarantees the cell can only be unlocked after `finality blocks`.

```
struct WithdrawalLockArgs {
    withdrawal_block_hash: Byte32,
    withdrawal_finalized_timepoint: Uint64,
    account_script_hash: Byte32,
    // layer1 lock to withdraw after challenge period
    owner_lock_hash: Byte32,
}
```

`withdrawal_block_hash` and `withdrawal_finalized_timepoint` record which layer2 block included the withdrawal. `account_script_hash` represent the layer2 account. `owner_lock_hash` represent the layer1 lock that user used to unlock the cell.

CKB requires `capacity` to cover the cost of the cell, so the minimal withdrawal CKB that Godwoken allows is as follows:

* Withdrawal CKB: 266 CKB
* Withdrawal CKB and Simple UDT: 347 CKB

The layer-1 withdrawal cell are processed by the block producer, so users do not need to know the details, they submit the withdrawal request and wait for receiving the assets cell on CKB.

### Legacy withdrawal cells (v0)

The lagacy withdrawal cells are not used anymore on the Godwoken network.

The withdrawal cell:

``` yaml
lock:
  code_hash:    (withdrawal lock's code hash),
  hash_type:    Type,
  args: (rollup_type_hash(32 bytes) | WithdrawalLockArgsV0 (n bytes) | owner lock len (optional) | owner lock (optional) | withdrawal_to_v1 flag byte (optional)
capacity:   (CKB amount),
type_:  (none or SUDT script)
data:   (none or SUDT amount)
```

Withdrawal lock guarantees the cell can only be unlocked after `finality blocks`.

```
// --- withdrawal lock ---
// a rollup_type_hash exists before this args, to make args friendly to prefix search
struct WithdrawalLockArgsV0 {
    account_script_hash: Byte32,
    withdrawal_block_hash: Byte32,
    withdrawal_block_number: Uint64,
    // buyer can pay sell_amount token to unlock
    sudt_script_hash: Byte32,
    sell_amount: Uint128,
    sell_capacity: Uint64,
    // layer1 lock to withdraw after challenge period
    owner_lock_hash: Byte32,
    // layer1 lock to receive the payment, must exists on the chain
    payment_lock_hash: Byte32,
}
```

We have **optional** fields in the withdrawal cell's args:

* owner lock - If users submit withdrawal request with an owner lock structure. The block producer will generate withdrawal cells with `owner lock` field in the args, and automatically unlock these cells after they are finalized. Users don't need to manually unlock layer-1 withdrawal cells.
* withdrawal_to_v1 - This field only works when `owner lock` exist, if `withdrawal_to_v1` is exist and the value is `1`, which means the withdrawal is a fast withdrawal to Godwoken v1. A fast withdrawal from v0 to v1 can be instantly processed, and the assets will be migrated to Godwoken v1.

### **Note**:
manually withdrawl - If `owner lock` isn't exist, users must manually unlock the legacy withdrawal cell after it is finalized, and user must provides an input cell in the unlocking transaction that its `lock hash` is equals to withdrawal lock args' `owner_lock_hash`.
