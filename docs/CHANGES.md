# v1.0 CHANGES

Consider the further extension of the Godwoken, we use Godwoken internal addresses in the EVM instead of Ethereum addresses. This design causes some incompatiblity between Godwoken & Ethereum, to remove the incompability, we provide a web3-provider plugin to handling the conversion between the two types of address. We hope users can using Godwoken seamlessly as they are using Ethereum. However, even we can handle the conversion in mostly cases, we still have some conner cases: such as we cannot hanlde the conversion happend inside metamask or in some hardware wallets, we can't integrate web3 provider in these environments. In general, we think the legacy version(current mainnet) can solve 95% problems of the compatiblility, but it still leaves some problems need developers to solve it case by case, this downgrade the developer's experience.

In the v1.0 version, we fix this problem by storing the mapping relation of Ethereum address & Godwoken script hash into a builtin contract: Ethereum address registry. So in the new version, we totally remove the web3-provider, and directly handling ethereum address in the godwoken-web3 server. Developers can calling or deploy their contract without modification or using extra plugins such as web3-provider.

## Godwoken internal changes

(Note, this is about Godwoken internals, you can skip this if you are a Dapp developer)

### Core data structure

1. The deposit data structure is refactored, a field `registry_id` is added on the `DepositLockArgs` & `DepositRequest` to indicate which registry we deposited to (The `registry_id` should always using the Ethereum address registry in the current version).
2. The `RawWithdrawalRequest` data structure is refactored, a field `registry_id` is added on the structure to indicate which registry it withdraws from.
3. The `RawL2block#block_producer_id : Uint32` is change to `RawL2Block#block_producer : Bytes`

### Address registry

1. A new builtin contract Ethereum address registry is added, we can register or query addresses mapping from this contract.
2. Deposit automatically mapping the Ethereum address for the account. But if the account is created through Meta contract, the developer must register the Ethereum address for the acount by calling the Ethereum address registry contract.

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

### Fee

In v1.0 we remove the feature of paying tx fee with Simple UDT, now CKB is the only token used to pay tx fee.

### RPC

Details is in the RPC documentation `docs/RPC.md`

1. Remove `gw_get_script_hash_by_short_address`
2. Add `gw_get_registry_address_by_script_hash`
2. Add `gw_get_script_hash_by_registry_address`
3. Update `gw_get_balance`
