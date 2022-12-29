#ifndef GW_DEF_H_
#define GW_DEF_H_

#include "gw_registry_addr.h"
#include "stddef.h"
#include "uint256.h"

typedef unsigned __int128 uint128_t;

#define GW_KEY_BYTES 32
#define GW_VALUE_BYTES 32

/* Builtins */
#define GW_DEFAULT_ETH_REGISTRY_ACCOUNT_ID 2

/* Key type */
#define GW_ACCOUNT_KV 0
#define GW_ACCOUNT_NONCE 1
#define GW_ACCOUNT_SCRIPT_HASH 2
/* Non account type */
#define GW_ACCOUNT_SCRIPT_HASH_TO_ID 3
#define GW_DATA_HASH_PREFIX 4
/* Godwoken Registry key type */
#define GW_REGISTRY_KEY_FLAG_SCRIPT_HASH_TO_NATIVE 1
#define GW_REGISTRY_KEY_FLAG_NATIVE_TO_SCRIPT_HASH 2

/* Limitations */
/* GW_MAX_BLOCK_INFO_SIZE */
#define GW_MAX_BLOCK_INFO_SIZE 256
/* 25KB (ethereum max contract code size) */
#define GW_MAX_DATA_SIZE (25 * 1024)
#define GW_MAX_RETURN_DATA_SIZE (128 * 1024)
/* 128KB */
#define GW_MAX_L2TX_ARGS_SIZE (128 * 1024)
/* 128KB + 4KB */
#define GW_MAX_L2TX_SIZE (132 * 1024)
/* MAX kv state pairs in a tx */
#define GW_MAX_KV_PAIRS 1024
#define GW_MAX_SCRIPT_SIZE 256
/* MAX scripts in a tx */
#define GW_MAX_SCRIPT_ENTRIES_SIZE 100
/* MAX data hash can load using sys_load_data in a tx */
#define GW_MAX_LOAD_DATA_ENTRIES_SIZE 100
/* MAX size of rollup config */
#define GW_MAX_ROLLUP_CONFIG_SIZE (4 * 1024)
#define GW_MAX_WITNESS_SIZE (300 * 1024)

#define GW_LOG_SUDT_TRANSFER 0x0
#define GW_LOG_SUDT_PAY_FEE 0x1
#define GW_LOG_POLYJUICE_SYSTEM 0x2
#define GW_LOG_POLYJUICE_USER 0x3

#define GW_ALLOWED_EOA_UNKNOWN 0
#define GW_ALLOWED_EOA_ETH 1

#define GW_ALLOWED_CONTRACT_UNKNOWN 0
#define GW_ALLOWED_CONTRACT_META 1
#define GW_ALLOWED_CONTRACT_SUDT 2
#define GW_ALLOWED_CONTRACT_POLYJUICE 3
#define GW_ALLOWED_CONTRACT_ETH_ADDR_REG 4

/* Godwoken context */
typedef struct {
  uint32_t from_id;
  uint32_t to_id;
  uint8_t args[GW_MAX_L2TX_ARGS_SIZE];
  uint32_t args_len;
} gw_transaction_context_t;

typedef struct {
  uint64_t number;
  uint64_t timestamp;
  gw_reg_addr_t block_producer;
} gw_block_info_t;

struct gw_context_t;

/**
 * Initialize Godwoken context
 */
int gw_context_init(struct gw_context_t *ctx);

/**
 * Finalize Godwoken state
 */
int gw_finalize(struct gw_context_t *ctx);

/**
 * Verify sudt account
 */
int gw_verify_sudt_account(struct gw_context_t *ctx, uint32_t sudt_id);

/* layer2 syscalls */

/**
 * Create a new account
 *
 * @param ctx        The godwoken context
 * @param script     Contract's script (MUST be valid molecule format CKB
 * Script)
 * @param script_len Length of script structure
 * @param account_id ID of new account
 * @return           The status code, 0 is success
 */
typedef int (*gw_create_fn)(struct gw_context_t *ctx, uint8_t *script,
                            uint64_t script_len, uint32_t *account_id);

/**
 * @param input  two curve points (x, y)
 * @param output curve point x + y, where + is point addition on the elliptic
 * curve
 *
 * Fails on invalid input
 */
typedef int (*gw_bn_add)(const uint8_t *input, const size_t input_size,
                         uint8_t *output);

/**
 * @param Input  two curve points (x, y)
 * @param output curve point s * x, where * is the scalar multiplication on the
 * elliptic curve
 *
 * Fails on invalid input
 */
typedef int (*gw_bn_mul)(const uint8_t *input, const size_t input_size,
                         uint8_t *output);

/**
 * @param input  Input: (a1, b1, a2, b2, ..., ak, bk) from (G_1 x G_2)^k
 *               Note that k is the input_size divided by 192
 * @param output curve point s * x, where * is the scalar multiplication on the
 * elliptic curve
 *
 * @return Empty input is valid and results in returning one.
 *
 * Fails on:
 *   1. the input_size is not a multiple of 192
 *   2. any of the inputs are not elements of the respective group are not
 * encoded correctly
 */
typedef int (*gw_bn_pairing)(const uint8_t *input, const size_t input_size,
                             uint8_t *output);

/**
 * Load value by key from current contract account
 *
 * @param ctx        The godwoken context
 * @param account_id account to modify
 * @param key        The key (less than 32 bytes)
 * @param key_len    The key length (less then 32)
 * @param value      The pointer to save the value of the key (32 bytes)
 * @return           The status code, 0 is success
 */
typedef int (*gw_load_fn)(struct gw_context_t *ctx, uint32_t account_id,
                          const uint8_t *key, const uint64_t key_len,
                          uint8_t value[GW_VALUE_BYTES]);

/**
 * Store key,value pair to current account's storage
 *
 * @param ctx        The godwoken context
 * @param account_id account to read
 * @param key        The key (less than 32 bytes)
 * @param key_len    The key length (less then 32)
 * @param value      The value
 * @return           The status code, 0 is success
 */
typedef int (*gw_store_fn)(struct gw_context_t *ctx, uint32_t account_id,
                           const uint8_t *key, const uint64_t key_len,
                           const uint8_t value[GW_VALUE_BYTES]);

/**
 * Set the return data of current layer 2 contract (program) execution
 *
 * @param data   The data to return
 * @param len    The length of return data
 * @return       The status code, 0 is success
 */
typedef int (*gw_set_program_return_data_fn)(struct gw_context_t *ctx,
                                             uint8_t *data, uint64_t len);

/**
 * Get account id by account script_hash
 *
 * @param ctx        The godwoken context
 * @param script_hashThe account script_hash
 * @param account_id The pointer of the account id to save the result
 * @return           The status code, 0 is success
 */
typedef int (*gw_get_account_id_by_script_hash_fn)(struct gw_context_t *ctx,
                                                   uint8_t script_hash[32],
                                                   uint32_t *account_id);

/**
 * Get account script_hash by account id
 *
 * @param ctx        The godwoken context
 * @param account_id The account id
 * @param script_hashThe pointer of the account script hash to save the result
 * @return           The status code, 0 is success
 */
typedef int (*gw_get_script_hash_by_account_id_fn)(struct gw_context_t *ctx,
                                                   uint32_t account_id,
                                                   uint8_t script_hash[32]);

/**
 * Get account's nonce
 *
 * @param ctx        The godwoken context
 * @param account_id The account id
 * @param nonce      The point of the nonce to save the result
 * @return           The status code, 0 is success
 */
typedef int (*gw_get_account_nonce_fn)(struct gw_context_t *ctx,
                                       uint32_t account_id, uint32_t *nonce);

/**
 * Get account script by account id
 */
typedef int (*gw_get_account_script_fn)(struct gw_context_t *ctx,
                                        uint32_t account_id, uint64_t *len,
                                        uint64_t offset, uint8_t *script);
/**
 * Load data by data hash
 *
 * @param ctx        The godwoken context
 * @param data_hash  The data hash (hash = ckb_blake2b(data))
 * @param len        The length of the script data
 * @param offset     The offset of the script data
 * @param data       The pointer of the data to save the result
 * @return           The status code, 0 is success
 */
typedef int (*gw_load_data_fn)(struct gw_context_t *ctx, uint8_t data_hash[32],
                               uint64_t *len, uint64_t offset, uint8_t *data);

typedef int (*gw_store_data_fn)(struct gw_context_t *ctx, uint64_t data_len,
                                uint8_t *data);

/**
 * Get layer 2 block hash by number
 *
 * @param ctx        The godwoken context
 * @param number     The number of the layer 2 block
 * @param block_hash The pointer of the layer 2 block hash to save the result
 * @return           The status code, 0 is success
 */
typedef int (*gw_get_block_hash_fn)(struct gw_context_t *ctx, uint64_t number,
                                    uint8_t block_hash[32]);

/**
 * Recover an EoA account script by signature
 *
 * @param ctx            The godwoken context
 * @param message        The message of corresponding signature
 * @param signature      The pointer of signature data
 * @param signature_len  The length of signature data
 * @param code_hash      The EoA account script's code_hash
 * @param script         The pointer of script data
 * @param script_len     The pointer to length of script data
 * @return               The status code, 0 is success
 */

typedef int (*gw_recover_account_fn)(struct gw_context_t *ctx,
                                     uint8_t message[32], uint8_t *signature,
                                     uint64_t signature_len,
                                     uint8_t code_hash[32], uint8_t *script,
                                     uint64_t *script_len);

/**
 * Emit a log (EVM LOG0, LOG1, LOGn in polyjuice)
 *
 * @param ctx            The godwoken context
 * @param account_id     The account to emit log
 * @param service_flag   The service flag of log, for category different log
 * types
 * @param data           The log data
 * @param data_length    The length of the log data
 * @return               The status code, 0 is success
 */
typedef int (*gw_log_fn)(struct gw_context_t *ctx, uint32_t account_id,
                         uint8_t service_flag, uint64_t data_length,
                         const uint8_t *data);

/**
 * Record fee payment
 *
 * @param payer_addr                  Registry address
 * @param sudt_id                     Account id of sUDT
 * @param amount                      The amount of fee
 * @return                            The status code, 0 is success
 */
typedef int (*gw_pay_fee_fn)(struct gw_context_t *ctx, gw_reg_addr_t payer_addr,
                             uint32_t sudt_id, uint256_t amount);

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

/**
 * Create snapshot of state
 *
 * @param ctx        The godwoken context
 * @param snapshot_id The pointer of the snapshot id to save the result
 * @return           The status code, 0 is success
 */
typedef int (*gw_snapshot_fn)(struct gw_context_t *ctx, uint32_t *snapshot_id);

/**
 * Revert state
 *
 * @param ctx        The godwoken context
 * @param snapshot_id The snapshot id
 * @return           The status code, 0 is success
 */
typedef int (*gw_revert_fn)(struct gw_context_t *ctx, uint32_t snapshot_id);

/**
 * Revert state
 *
 * @param ctx        The godwoken context
 * @param sudt_proxy_addr The address of sudt proxy contract
 * @return           The status code, 0 is success
 */
typedef int (*gw_check_sudt_addr_permission_fn)(
    struct gw_context_t *ctx, const uint8_t sudt_proxy_addr[20]);

/**
 * Load value by raw key from state tree
 *
 * @param ctx        The godwoken context
 * @param raw_key        The key (less than 32 bytes)
 * @param key_len    The key length (less then 32)
 * @param value      The pointer to save the value of the key (32 bytes)
 * @return           The status code, 0 is success
 */
typedef int (*_gw_load_raw_fn)(struct gw_context_t *ctx,
                               const uint8_t raw_key[GW_KEY_BYTES],
                               uint8_t value[GW_VALUE_BYTES]);

/**
 * Store key,value pair to state tree
 *
 * @param ctx        The godwoken context
 * @param account_id account to read
 * @param key        The key (less than 32 bytes)
 * @param key_len    The key length (less then 32)
 * @param value      The value
 * @return           The status code, 0 is success
 */
typedef int (*_gw_store_raw_fn)(struct gw_context_t *ctx,
                                const uint8_t raw_key[GW_KEY_BYTES],
                                const uint8_t value[GW_VALUE_BYTES]);

#endif /* GW_DEF_H_ */
