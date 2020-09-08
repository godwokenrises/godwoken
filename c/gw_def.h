#ifndef GW_DEF_H_
#define GW_DEF_H_

#include "stddef.h"

#define GW_KEY_BYTES 32
#define GW_VALUE_BYTES 32

/* Key type */
#define GW_ACCOUNT_KV 0
#define GW_ACCOUNT_NONCE 1
#define GW_ACCOUNT_PUBKEY_HASH 2
#define GW_ACCOUNT_CODE_HASH 3

#define GW_MAX_RETURN_DATA_SIZE 1024

/* Call receipt */
typedef struct {
  uint8_t return_data[GW_MAX_RETURN_DATA_SIZE];
  uint32_t return_data_len;
} gw_call_receipt_t;

/* layer2 syscalls */
typedef int (*gw_load_fn)(void *ctx, const uint8_t key[GW_KEY_BYTES],
                          uint8_t value[GW_VALUE_BYTES]);
typedef int (*gw_store_fn)(void *ctx, const uint8_t key[GW_KEY_BYTES],
                           const uint8_t value[GW_VALUE_BYTES]);
typedef int (*gw_set_return_data_fn)(void *ctx, uint8_t *data, uint32_t len);
typedef int (*gw_call_fn)(void *ctx, uint32_t account_id, uint8_t *args,
                          uint32_t args_len, gw_call_receipt_t *receipt);

/* Blake2b hash function wrapper */
typedef void (*gw_blake2b_hash_fn)(uint8_t output_hash[GW_KEY_BYTES],
                                   uint8_t *input_data, uint32_t len);

/* Godwoken context */
typedef struct {
  uint32_t from_id;
  uint32_t to_id;
  // 0: construct, 1: handle_message
  uint8_t call_type;
  uint8_t *args;
  uint32_t args_len;
} gw_call_context_t;

typedef struct {
  uint64_t number;
  uint64_t timestamp;
  uint32_t aggregator_id;
} gw_block_info_t;

typedef struct {
  /* verification context */
  gw_call_context_t call_context;
  gw_block_info_t block_info;
  /* layer2 syscalls */
  void *sys_context;
  gw_load_fn sys_load;
  gw_store_fn sys_store;
  gw_set_return_data_fn sys_set_return_data;
  gw_call_fn sys_call;
  /* blake2b hash function helper */
  gw_blake2b_hash_fn blake2b_hash;
  /* Code buffer */
  uint8_t *code_buffer;
  uint32_t code_buffer_len;
  uint32_t code_buffer_used_size;
} gw_context_t;

/* layer2 contract interfaces */
typedef int (*gw_contract_fn)(gw_context_t *);

#endif /* GW_DEF_H_ */