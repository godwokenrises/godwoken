#ifndef GW_DEF_H_
#define GW_DEF_H_

#include "stddef.h"

#define GW_KEY_BYTES 32
#define GW_VALUE_BYTES 32

/* Key type */
#define GW_ACCOUNT_KV 0
#define GW_ACCOUNT_NONCE 1
#define GW_ACCOUNT_SCRIPT_HASH 2
#define GW_ACCOUNT_SCRIPT_HASH_TO_ID 3

/* 24KB (ethereum max contract code size) */
#define GW_MAX_RETURN_DATA_SIZE 24576
/* 128KB */
#define GW_MAX_ARGS_SIZE 131072
/* Buffer to receive data */
#define MAX_BUF_SIZE 65536
/* 2048 * (32 + 32 + 8) = 147456 Byte (~144KB)*/
#define MAX_KV_STATE_CAPACITY 2048

/* Limitations */
#define GW_MAX_KV_PAIRS 1024
#define GW_SCRIPT_SIZE 128
#define GW_MAX_WITNESS_SIZE (300 * 1024)
#define GW_MAX_CODE_SIZE (512 * 1024)

/* Godwoken context */
typedef struct {
  uint32_t from_id;
  uint32_t to_id;
  uint8_t args[GW_MAX_ARGS_SIZE];
  uint32_t args_len;
} gw_transaction_context_t;

typedef struct {
  uint64_t number;
  uint64_t timestamp;
  uint32_t aggregator_id;
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
typedef int (*gw_create_fn)(struct gw_context_t *ctx,
                            uint8_t *script,
                            uint32_t script_len,
                            uint32_t *account_id);

/**
 * Load value by key from current contract account
 *
 * @param ctx    The godwoken context
 * @param account_id  account to modify
 * @param key    The key (32 bytes)
 * @param value  The pointer to save the value of the key (32 bytes)
 * @return       The status code, 0 is success
 */
typedef int (*gw_load_fn)(struct gw_context_t *ctx,
                          uint32_t account_id,
                          const uint8_t key[GW_KEY_BYTES],
                          uint8_t value[GW_VALUE_BYTES]);
/**
 * Load the nonce of account
 *
 * @param ctx         The godwoken context
 * @param account_id  The account to load nonce
 * @param value       The pointer to save the nonce value of the key (32 bytes)
 * @return            The status code, 0 is success
 */
typedef int (*gw_load_nonce_fn)(struct gw_context_t *ctx,
                                uint32_t account_id,
                                uint8_t value[GW_VALUE_BYTES]);
/**
 * Increase the nonce of account by 1
 *
 * @param ctx         The godwoken context
 * @param account_id  The account to increase nonce
 * @param new_nonce   The pointer to new nonce (can be NULL)
 * @return            The status code, 0 is success
 */
typedef int (*gw_increase_nonce_fn)(struct gw_context_t *ctx,
                                    uint32_t account_id,
                                    uint32_t *new_nonce);


/**
 * Store key,value pair to current account's storage
 *
 * @param ctx    The godwoken context
 * @param account_id  account to read
 * @param key    The key
 * @param value  The value
 * @return       The status code, 0 is success
 */
typedef int (*gw_store_fn)(struct gw_context_t *ctx,
                           uint32_t account_id,
                           const uint8_t key[GW_KEY_BYTES],
                           const uint8_t value[GW_VALUE_BYTES]);

/**
 * Set the return data of current layer 2 contract (program) execution
 *
 * @param data   The data to return
 * @param len    The length of return data
 * @return       The status code, 0 is success
 */
typedef int (*gw_set_program_return_data_fn)(struct gw_context_t *ctx,
                                             uint8_t *data,
                                             uint32_t len);

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
                                       uint32_t account_id,
                                       uint32_t *nonce);

/**
 * Get account script by account id
 */
typedef int (*gw_get_account_script_fn)(struct gw_context_t *ctx,
                                        uint32_t account_id,
                                        uint32_t *len,
                                        uint32_t offset,
                                        uint8_t *script);
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
typedef int (*gw_load_data_fn)(struct gw_context_t *ctx,
                               uint8_t data_hash[32],
                               uint32_t *len,
                               uint32_t offset,
                               uint8_t *data);

typedef int (*gw_store_data_fn)(struct gw_context_t *ctx,
                                uint32_t data_len,
                                uint8_t *data);

/**
 * Get layer 2 block hash by number
 *
 * @param ctx        The godwoken context
 * @param block_hash The pointer of the layer 2 block hash to save the result
 * @param number     The number of the layer 2 block
 * @return           The status code, 0 is success
 */
typedef int (*gw_get_block_hash_fn)(struct gw_context_t *ctx,
                                    uint64_t number,
                                    uint8_t block_hash[32]);

/**
 * Emit a log (EVM LOG0, LOG1, LOGn in polyjuice)
 *
 * @param ctx            The godwoken context
 * @param account_id     The account to emit log
 * @param data           The log data
 * @param data_length    The length of the log data
 * @return               The status code, 0 is success
 */
typedef int (*gw_log_fn)(struct gw_context_t *ctx,
                         uint32_t account_id,
                         uint32_t data_length,
                         const uint8_t *data);

#endif /* GW_DEF_H_ */
