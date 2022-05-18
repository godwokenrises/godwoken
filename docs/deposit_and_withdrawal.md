# Deposit and withdrawal

Deposit and withdrawal is a special layer1 <-> layer2 messaging mechanism with assets transfer.

## Deposit

User deposit by sending a layer1 transaction on CKB while generating a cell with a special lock - deposit lock, which allows the block producer to unlock the cell. The block producer unlocks the deposited cell and then moves the assets under the custodian lock, at the same time, the block producer writes the deposit assets to layer2 state. All the process is enforced by layer1 script, even the block producer can't take the assets away.

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

The deposit cell is converted into a custodian cell, which means the assets are locked by layer2, custodian lock enforced the safety of the custodian assets, the assets can only be transferred out when the user withdrawal.

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
    deposit_block_number: Uint64,
    deposit_lock_args: DepositLockArgs,
}
```

`CustodianLockArgs` saves the entire deposit info, `deposit_lock_args` is from the original deposit cell's args, `deposit_block_hash` and `deposit_block_number` denotes the layer2 block that include the deposit.

CKB requires `capacity` to cover the cost of the cell, the `capacity` of the deposited cell must also cover the custodian cell, so the minimal deposit CKB that Godwoken allows is as follows:

* Deposit CKB: 246 CKB
* Deposit CKB and Simple UDT: 327 CKB


## Withdrawal

Withdrawal is the reverse process of the deposit. A user signs a withdrawal request and sends it to the block producer, block producer converts the custodian cell into a withdrawal cell and updates the layer2 state.

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

Withdrawal lock guarantees the user can unlock this cell after `finality blocks`.

```
struct WithdrawalLockArgs {
    withdrawal_block_hash: Byte32,
    withdrawal_block_number: Uint64,
    account_script_hash: Byte32,
    // layer1 lock to withdraw after challenge period
    owner_lock_hash: Byte32,
}
```

`withdrawal_block_hash` and `withdrawal_block_number` record the layer2 block including the withdrawal. `account_script_hash` denotes the layer2 account. `owner_lock_hash` denotes the layer1 lock user used to unlock the cell.

CKB requires `capacity` to cover the cost of the cell, so the minimal withdrawal CKB that Godwoken allows is as follows:

* Withdrawal CKB: 234 CKB
* Withdrawal CKB and Simple UDT: 315 CKB
