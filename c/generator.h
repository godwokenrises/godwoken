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
#define GW_SYS_STORE_ACCOUNT_CODE 4056
#define GW_SYS_LOAD_ACCOUNT_CODE 4057
#define GW_SYS_LOG 4061

#define MAX_BUF_SIZE 65536

int sys_load(void *ctx, uint32_t account_id, const uint8_t key[GW_KEY_BYTES],
             uint8_t value[GW_VALUE_BYTES]) {
  gw_context_t *gw_ctx = (gw_context_t *)ctx;
  if (gw_ctx == NULL) {
    return GW_ERROR_INVALID_CONTEXT;
  }
  uint8_t raw_key[GW_KEY_BYTES] = {0};
  gw_build_account_key(account_id, key, raw_key);
  return syscall(GW_SYS_LOAD, raw_key, value, 0, 0, 0, 0);
}
int sys_store(void *ctx, uint32_t account_id, const uint8_t key[GW_KEY_BYTES],
              const uint8_t value[GW_VALUE_BYTES]) {
  gw_context_t *gw_ctx = (gw_context_t *)ctx;
  if (gw_ctx == NULL) {
    return GW_ERROR_INVALID_CONTEXT;
  }
  uint8_t raw_key[GW_KEY_BYTES];
  gw_build_account_key(account_id, key, raw_key);
  return syscall(GW_SYS_STORE, raw_key, value, 0, 0, 0, 0);
}

int sys_load_nonce(void *ctx, uint32_t account_id, uint8_t value[GW_VALUE_BYTES]) {
  uint8_t key[32];
  gw_build_nonce_key(account_id, key);
  return syscall(GW_SYS_LOAD, key, value, 0, 0, 0, 0);
}

/* set call return data */
int sys_set_program_return_data(void *ctx, uint8_t *data, uint32_t len) {
  return syscall(GW_SYS_SET_RETURN_DATA, data, len, 0, 0, 0, 0);
}

/* Get account id by account script_hash */
int sys_get_account_id_by_script_hash(void *ctx, uint8_t script_hash[32],
                                      uint32_t *account_id) {
  return syscall(GW_SYS_LOAD_ACCOUNT_ID_BY_SCRIPT_HASH, script_hash, account_id,
                 0, 0, 0, 0);
}

/* Get account script_hash by account id */
int sys_get_script_hash_by_account_id(void *ctx, uint32_t account_id,
                                      uint8_t script_hash[32]) {
  return syscall(GW_SYS_LOAD_SCRIPT_HASH_BY_ACCOUNT_ID, account_id, script_hash,
                 0, 0, 0, 0);
}

/* Get account script by account id */
int sys_get_account_script(void *ctx, uint32_t account_id, uint32_t *len,
                         uint32_t offset, uint8_t *script) {
  return syscall(GW_SYS_LOAD_ACCOUNT_SCRIPT, account_id, len, offset, script, 0, 0);
}
/* Store account code by account id */
int sys_set_account_code(void *ctx,
                         uint32_t account_id,
                         uint32_t code_len,
                         uint8_t *code) {
  return syscall(GW_SYS_STORE_ACCOUNT_CODE, account_id, code_len, code, 0, 0, 0);
}
/* Load account code by account id */
int sys_get_account_code(void *ctx, uint32_t account_id, uint32_t *len,
                           uint32_t offset, uint8_t *code) {
  return syscall(GW_SYS_LOAD_ACCOUNT_CODE, account_id, len, offset, code, 0, 0);
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

int sys_create(void *ctx, uint8_t *script, uint32_t script_len,
               uint32_t *account_id) {
  return syscall(GW_SYS_CREATE, script, script_len, account_id, 0, 0, 0);
}

int sys_log(void *ctx, uint32_t account_id, uint32_t data_length,
            const uint8_t *data) {
  return syscall(GW_SYS_LOG, account_id, data_length, data, 0, 0, 0);
}

int gw_context_init(gw_context_t *context) {
  memset(context, 0, sizeof(gw_context_t));
  /* setup syscalls */
  context->sys_load = sys_load;
  context->sys_load_nonce = sys_load_nonce;
  context->sys_store = sys_store;
  context->sys_set_program_return_data = sys_set_program_return_data;
  context->sys_create = sys_create;
  context->sys_get_account_id_by_script_hash =
      sys_get_account_id_by_script_hash;
  context->sys_get_script_hash_by_account_id =
      sys_get_script_hash_by_account_id;
  context->sys_get_account_script = sys_get_account_script;
  context->sys_set_account_code = sys_set_account_code;
  context->sys_get_account_code = sys_get_account_code;
  context->sys_log = sys_log;

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
  ret = gw_parse_transaction_context(&context->transaction_context,
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
  ret = gw_parse_block_info(&context->block_info, &block_info_seg);
  if (ret != 0) {
    return ret;
  }

  return 0;
}

#endif
