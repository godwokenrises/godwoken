#ifndef GW_GENERATOR_H_
#define GW_GENERATOR_H_
/* Layer2 contract generator
 *
 * The generator supposed to be run off-chain.
 * generator dynamic linking with the layer2 contract code,
 * and provides layer2 syscalls.
 *
 * A program should be able to generate a post state after run the generator,
 * and should be able to use the states to construct a transaction that satifies
 * the validator.
 */

#include "ckb_syscalls.h"
#include "common.h"

/* syscalls */
#define GW_SYS_STORE 3051
#define GW_SYS_LOAD 3052
#define GW_SYS_SET_RETURN_DATA 3061
#define GW_SYS_CREATE 3071
/* internal syscall only for generator */
#define GW_SYS_LOAD_TRANSACTION 4051
#define GW_SYS_LOAD_BLOCKINFO 4052
#define GW_SYS_LOAD_SCRIPT_HASH_BY_ACCOUNT_ID 4053
#define GW_SYS_LOAD_ACCOUNT_ID_BY_SCRIPT_HASH 4054
#define GW_SYS_LOAD_ACCOUNT_SCRIPT 4055
#define GW_SYS_STORE_DATA 4056
#define GW_SYS_LOAD_DATA 4057
#define GW_SYS_LOG 4061

#define MAX_BUF_SIZE 65536

typedef struct gw_context_t {
  /* verification context */
  gw_transaction_context_t transaction_context;
  gw_block_info_t block_info;
  /* layer2 syscalls */
  gw_load_fn sys_load;
  gw_load_nonce_fn sys_load_nonce;
  gw_increase_nonce_fn sys_increase_nonce;
  gw_store_fn sys_store;
  gw_set_program_return_data_fn sys_set_program_return_data;
  gw_create_fn sys_create;
  gw_get_account_id_by_script_hash_fn sys_get_account_id_by_script_hash;
  gw_get_script_hash_by_account_id_fn sys_get_script_hash_by_account_id;
  gw_get_account_nonce_fn sys_get_account_nonce;
  gw_get_account_script_fn sys_get_account_script;
  gw_load_data_fn sys_load_data;
  gw_store_data_fn sys_store_data;
  gw_get_block_hash_fn sys_get_block_hash;
  gw_log_fn sys_log;
} gw_context_t;


int sys_load(gw_context_t *ctx, uint32_t account_id, const uint8_t key[GW_KEY_BYTES],
             uint8_t value[GW_VALUE_BYTES]) {
  gw_context_t *gw_ctx = (gw_context_t *)ctx;
  if (gw_ctx == NULL) {
    return GW_ERROR_INVALID_CONTEXT;
  }
  uint8_t raw_key[GW_KEY_BYTES] = {0};
  gw_build_account_key(account_id, key, raw_key);
  return syscall(GW_SYS_LOAD, raw_key, value, 0, 0, 0, 0);
}
int sys_store(gw_context_t *ctx, uint32_t account_id, const uint8_t key[GW_KEY_BYTES],
              const uint8_t value[GW_VALUE_BYTES]) {
  gw_context_t *gw_ctx = (gw_context_t *)ctx;
  if (gw_ctx == NULL) {
    return GW_ERROR_INVALID_CONTEXT;
  }
  uint8_t raw_key[GW_KEY_BYTES];
  gw_build_account_key(account_id, key, raw_key);
  return syscall(GW_SYS_STORE, raw_key, value, 0, 0, 0, 0);
}

int sys_load_nonce(gw_context_t *ctx, uint32_t account_id, uint8_t value[GW_VALUE_BYTES]) {
  uint8_t key[32];
  gw_build_nonce_key(account_id, key);
  return syscall(GW_SYS_LOAD, key, value, 0, 0, 0, 0);
}

int sys_increase_nonce(gw_context_t *ctx, uint32_t account_id, uint32_t *new_nonce) {
  uint8_t old_nonce_value[GW_VALUE_BYTES];
  int ret = sys_load_nonce(ctx, account_id, old_nonce_value);
  if (ret != 0) {
    return ret;
  }
  for (size_t i = 4; i < GW_VALUE_BYTES; i++) {
    if(old_nonce_value[i] != 0){
      return GW_ERROR_INVALID_DATA;
    }
  }
  uint32_t next_nonce = *((uint32_t *)old_nonce_value) + 1;

  uint8_t nonce_key[GW_KEY_BYTES];
  uint8_t nonce_value[GW_VALUE_BYTES];
  memset(nonce_value, 0, GW_VALUE_BYTES);
  gw_build_nonce_key(account_id, nonce_key);
  memcpy(nonce_value, (uint8_t *)(&next_nonce), 4);
  ret = syscall(GW_SYS_STORE, nonce_key, nonce_value, 0, 0, 0, 0);
  if (ret != 0) {
    return ret;
  }
  if (new_nonce != NULL) {
    *new_nonce = next_nonce;
  }
  return 0;
}

/* set call return data */
int sys_set_program_return_data(gw_context_t *ctx, uint8_t *data, uint32_t len) {
  return syscall(GW_SYS_SET_RETURN_DATA, data, len, 0, 0, 0, 0);
}

/* Get account id by account script_hash */
int sys_get_account_id_by_script_hash(gw_context_t *ctx, uint8_t script_hash[32],
                                      uint32_t *account_id) {
  return syscall(GW_SYS_LOAD_ACCOUNT_ID_BY_SCRIPT_HASH, script_hash, account_id,
                 0, 0, 0, 0);
}

/* Get account script_hash by account id */
int sys_get_script_hash_by_account_id(gw_context_t *ctx, uint32_t account_id,
                                      uint8_t script_hash[32]) {
  return syscall(GW_SYS_LOAD_SCRIPT_HASH_BY_ACCOUNT_ID, account_id, script_hash,
                 0, 0, 0, 0);
}

/* Get account script by account id */
int sys_get_account_script(gw_context_t *ctx, uint32_t account_id, uint32_t *len,
                         uint32_t offset, uint8_t *script) {
  return syscall(GW_SYS_LOAD_ACCOUNT_SCRIPT, account_id, len, offset, script, 0, 0);
}
/* Store data by data hash */
int sys_store_data(gw_context_t *ctx,
                 uint32_t data_len,
                 uint8_t *data) {
  return syscall(GW_SYS_STORE_DATA, data_len, data, 0, 0, 0, 0);
}
/* Load data by data hash */
int sys_load_data(gw_context_t *ctx, uint8_t data_hash[32],
                 uint32_t *len, uint32_t offset, uint8_t *data) {
  return syscall(GW_SYS_LOAD_DATA, data_hash, len, offset, data, 0, 0);
}

int _sys_load_l2transaction(void *addr, uint64_t *len) {
  volatile uint64_t inner_len = *len;
  int ret = syscall(GW_SYS_LOAD_TRANSACTION, addr, &inner_len, 0, 0, 0, 0);
  *len = inner_len;
  return ret;
}

int _sys_load_block_info(void *addr, uint64_t *len) {
  volatile uint64_t inner_len = *len;
  int ret = syscall(GW_SYS_LOAD_BLOCKINFO, addr, &inner_len, 0, 0, 0, 0);
  *len = inner_len;
  return ret;
}

int sys_create(gw_context_t *ctx, uint8_t *script, uint32_t script_len,
               uint32_t *account_id) {
  return syscall(GW_SYS_CREATE, script, script_len, account_id, 0, 0, 0);
}

int sys_log(gw_context_t *ctx, uint32_t account_id, uint32_t data_length,
            const uint8_t *data) {
  return syscall(GW_SYS_LOG, account_id, data_length, data, 0, 0, 0);
}

int gw_context_init(gw_context_t *ctx) {
  /* setup syscalls */
  ctx->sys_load = sys_load;
  ctx->sys_load_nonce = sys_load_nonce;
  ctx->sys_increase_nonce = sys_increase_nonce;
  ctx->sys_store = sys_store;
  ctx->sys_set_program_return_data = sys_set_program_return_data;
  ctx->sys_create = sys_create;
  ctx->sys_get_account_id_by_script_hash =
      sys_get_account_id_by_script_hash;
  ctx->sys_get_script_hash_by_account_id =
      sys_get_script_hash_by_account_id;
  ctx->sys_get_account_script = sys_get_account_script;
  ctx->sys_store_data = sys_store_data;
  ctx->sys_load_data = sys_load_data;
  ctx->sys_log = sys_log;

  /* initialize context */
  uint8_t buf[MAX_BUF_SIZE] = {0};
  uint64_t len = MAX_BUF_SIZE;
  int ret = _sys_load_l2transaction(buf, &len);
  if (ret != 0) {
    return ret;
  }
  if (len > MAX_BUF_SIZE) {
    return GW_ERROR_INVALID_DATA;
  }

  mol_seg_t l2transaction_seg;
  l2transaction_seg.ptr = buf;
  l2transaction_seg.size = len;
  ret = gw_parse_transaction_context(&ctx->transaction_context,
                                     &l2transaction_seg);
  if (ret != 0) {
    return ret;
  }

  len = MAX_BUF_SIZE;
  ret = _sys_load_block_info(buf, &len);
  if (ret != 0) {
    return ret;
  }
  if (len > MAX_BUF_SIZE) {
    return GW_ERROR_INVALID_DATA;
  }

  mol_seg_t block_info_seg;
  block_info_seg.ptr = buf;
  block_info_seg.size = len;
  ret = gw_parse_block_info(&ctx->block_info, &block_info_seg);
  if (ret != 0) {
    return ret;
  }

  return 0;
}

int gw_finalize(gw_context_t *ctx) {
  /* do nothing */
  return 0;
}
#endif
