# v1 release note

In Godwoken v0, Godwoken EVM was using an internal address format. Users need to integrate web3-provider to handling the conversion between the internal address format and the Ethereum address format. Although in most cases the conversion is handled transparently, this incompatibility still causes problems in some corner cases. Such as, we cannot handle the conversion in metamask or some hardware wallets because the web3-provider plugin is unsupported in these environments.

In the v1 version, we add a new builtin contract: Ethereum address registry. When a new account is created by user deposit, an Ethereum address is inserted to the contract. With the new builtin contract, the Ethereum address is supported in the EVM directly. Now users can submit their transaction to Godwoken without using the web3-provider plugin. We aim to provide 100% compatibility at the RPC level. When users switch to the Godwoken network, they can use Ethereum Dapps or toolchains without modification.

## Godwoken internal changes

(Note, this is about Godwoken internals, you can skip this if you are a Dapp developer)

### Core data structure

* `RollupConfig#compatible_chain_id` is changed to `RollupConfig#chain_id`. In v1 we use `RollupConfig#chain_id` as the `chain_id` in the signature directly.

``` molecule
table RollupConfig {
    l1_sudt_script_type_hash: Byte32,
    custodian_script_type_hash: Byte32,
    deposit_script_type_hash: Byte32,
    withdrawal_script_type_hash: Byte32,
    challenge_script_type_hash: Byte32,
    stake_script_type_hash: Byte32,
    l2_sudt_validator_script_type_hash: Byte32,
    burn_lock_hash: Byte32,
    required_staking_capacity: Uint64,
    challenge_maturity_blocks: Uint64,
    finality_blocks: Uint64,
    reward_burn_rate: byte, // * reward_burn_rate / 100
    chain_id: Uint64, // chain id
    allowed_eoa_type_hashes: AllowedTypeHashVec, // list of script code_hash allowed an EOA(external owned account) to use
    allowed_contract_type_hashes: AllowedTypeHashVec, // list of script code_hash allowed a contract account to use
}
```

* A field `registry_id` is added on the `DepositLockArgs` & `DepositRequest` to indicate which registry we deposited to (The `registry_id` should always be the builtin Ethereum address registry in v1).

``` molecule
table DepositLockArgs {
    owner_lock_hash: Byte32,
    layer2_lock: Script,
    cancel_timeout: Uint64,
    registry_id: Uint32,
}

table DepositRequest {
    // CKB amount
    capacity: Uint64,
    // SUDT amount
    amount: Uint128,
    sudt_script_hash: Byte32,
    script: Script,
    // Deposit to a Godwoken registry
    registry_id: Uint32,
}
```

* In `RawWithdrawalRequest`, a field `registry_id` is added to indicate which registry it withdraws from.
* `chain_id` is added to the structure to indicate the rollup chain_id.

``` molecule
struct RawWithdrawalRequest {
    nonce: Uint32,
    // chain id
    chain_id: Uint64,
    // CKB amount
    capacity: Uint64,
    // SUDT amount
    amount: Uint128,
    sudt_script_hash: Byte32,
    // layer2 account_script_hash
    account_script_hash: Byte32,
    // withdrawal registry ID
    registry_id: Uint32,
    // layer1 lock to withdraw after challenge period
    owner_lock_hash: Byte32,
    // withdrawal fee, paid to block producer
    fee: Uint64,
}
```

* `chain_id` is added to `RawL2Transaction` to indicate the rollup chain_id.

``` molecule
table RawL2Transaction {
    // chain id
    chain_id: Uint64,
    from_id: Uint32,
    to_id: Uint32,
    nonce: Uint32,
    args: Bytes,
}
```

* The `RawL2block#block_producer_id : Uint32` is change to `RawL2Block#block_producer : Bytes`, and the value is a serialized registry address.

``` molecule
table RawL2Block {
    number: Uint64,
    // In registry address format: registry_id (4 bytes) | address len (4 bytes) | address (n bytes)
    block_producer: Bytes,
    parent_block_hash: Byte32,
    stake_cell_owner_lock_hash: Byte32,
    timestamp: Uint64,
    prev_account: AccountMerkleState,
    post_account: AccountMerkleState,
    // hash(account_root | account_count) of each withdrawals & transactions
    state_checkpoint_list: Byte32Vec,
    submit_withdrawals: SubmitWithdrawals,
    submit_transactions: SubmitTransactions,
}
```

* Paying fee with Simple UDT is removed in v1, `registry_id` is added to `Fee` to indicate which registry the token is transfered to.

``` molecule
struct Fee {
    // registry id
    registry_id: Uint32,
    // amount in CKB
    amount: Uint64,
}
```

* Ethereum address registry data structures.

``` molecule
// --- ETH Address Registry ---
union ETHAddrRegArgs {
    EthToGw,
    GwToEth,
    SetMapping,
    BatchSetMapping,
}

struct EthToGw {
    eth_address: Byte20,
}

struct GwToEth {
    gw_script_hash: Byte32,
}

struct SetMapping {
    gw_script_hash: Byte32,
    fee: Fee,
}

table BatchSetMapping {
    gw_script_hashes: Byte32Vec,
    fee: Fee,
}

// --- end of ETH Address Registry ---
```

### Address registry

1. A new builtin contract Ethereum address registry is added. Godwoken uses it to handle the Ethereum address format. When a user deposits token to create a new account, a corresponding Ethereum address is inserted to the contract.
2. Deposit automatically maps the Ethereum addresses for new accounts. If the account is created through a Meta contract, the contract developer must register the Ethereum address for the acount by calling the Ethereum address registry contract.
3. The builtin Ethereum address registry is allocated to id 2 in the Godwoken genesis block.

### Godwoken syscalls

1. Remove `sys_get_script_hash_by_prefix_fn`
2. Update the `sys_pay_fee`, the second param is registry address.
3. Add `sys_get_registry_address_by_script_hash`
4. Add `sys_get_script_hash_by_registry_address`

``` c
/**
 * Record fee payment
 *
 * @param payer_addr                  Registry address
 * @param sudt_id                     Account id of sUDT
 * @param amount                      The amount of fee
 * @return                            The status code, 0 is success
 */
typedef int (*gw_pay_fee_fn)(struct gw_context_t *ctx, gw_reg_addr_t payer_addr,
                             uint32_t sudt_id, uint128_t amount);

/**
 * Get registry address by script_hash
 *
 * @param script_hash
 * @param reg_id registry_id
 * @param returned registry address
 * @return       The status code, 0 is success
 */
typedef int (*gw_get_registry_address_by_script_hash_fn)(
    struct gw_context_t *ctx, uint8_t script_hash[32], uint32_t reg_id,
    gw_reg_addr_t *address);

/**
 * Get script hash by address
 *
 * @param address
 * @param script_hash
 * @return       The status code, 0 is success
 */
typedef int (*gw_get_script_hash_by_registry_address_fn)(
    struct gw_context_t *ctx, gw_reg_addr_t *address, uint8_t script_hash[32]);
```

### Signing

In v0 we only support signing transactions with personal sign, this is considered insecure since users can only see a random 32-bytes hex, they have no idea what data they are signing.

V1 supports two signature formats:

1. The Ethereum transaction format, users can sign transactions in Metamask with the same experience in the ethereum world.
2. The EIP-712 format. We support using EIP-712 format to sign withdrawal messages or Godwoken transactions. Users can check the content of a transaction before they sign.

### Polyjuice

The polyjuice components using ethereum address in EVM execution.

### Fee

In v1.0 we remove the feature of paying tx fee with Simple UDT, now CKB is the only token to be used to pay tx fee.

### RPC

Details is in the RPC documentation `docs/RPC.md`

1. Remove `gw_get_script_hash_by_short_address`
2. Add `gw_get_registry_address_by_script_hash`
2. Add `gw_get_script_hash_by_registry_address`
3. Update `gw_get_balance`
