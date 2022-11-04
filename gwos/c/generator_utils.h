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
#include "gw_def.h"
#include "uint256.h"

/* syscalls */
/* Syscall account store / load / create */
#define GW_SYS_CREATE 3100
#define GW_SYS_STORE 3101
#define GW_SYS_LOAD 3102
#define GW_SYS_LOAD_ACCOUNT_SCRIPT 3105
/* Syscall call / return */
#define GW_SYS_SET_RETURN_DATA 3201
/* Syscall data store / load */
#define GW_SYS_STORE_DATA 3301
#define GW_SYS_LOAD_DATA 3302
/* Syscall load metadata structures */
#define GW_SYS_LOAD_ROLLUP_CONFIG 3401
#define GW_SYS_LOAD_TRANSACTION 3402
#define GW_SYS_LOAD_BLOCKINFO 3403
#define GW_SYS_GET_BLOCK_HASH 3404
/* Syscall builtins */
#define GW_SYS_PAY_FEE 3501
#define GW_SYS_LOG 3502
#define GW_SYS_RECOVER_ACCOUNT 3503
/* Syscall for make use the Barreto-Naehrig (BN) curve construction */
#define GW_SYS_BN_ADD 3601
#define GW_SYS_BN_MUL 3602
#define GW_SYS_BN_PAIRING 3603
/* Syscall state */
#define GW_SYS_SNAPSHOT 3701
#define GW_SYS_REVERT 3702

typedef struct gw_context_t {
  /* verification context */
  gw_transaction_context_t transaction_context;
  gw_block_info_t block_info;
  uint8_t rollup_config[GW_MAX_ROLLUP_CONFIG_SIZE];
  uint64_t rollup_config_size;
  /* original sender nonce */
  uint32_t original_sender_nonce;
  /* layer2 syscalls */
  gw_load_fn sys_load;
  gw_get_account_nonce_fn sys_get_account_nonce;
  gw_store_fn sys_store;
  gw_set_program_return_data_fn sys_set_program_return_data;
  gw_create_fn sys_create;
  gw_get_account_id_by_script_hash_fn sys_get_account_id_by_script_hash;
  gw_get_script_hash_by_account_id_fn sys_get_script_hash_by_account_id;
  gw_get_account_script_fn sys_get_account_script;
  gw_load_data_fn sys_load_data;
  gw_store_data_fn sys_store_data;
  gw_get_block_hash_fn sys_get_block_hash;
  gw_recover_account_fn sys_recover_account;
  gw_bn_add sys_bn_add;
  gw_bn_mul sys_bn_mul;
  gw_bn_pairing sys_bn_pairing;
  gw_log_fn sys_log;
  gw_pay_fee_fn sys_pay_fee;
  gw_get_registry_address_by_script_hash_fn
      sys_get_registry_address_by_script_hash;
  gw_get_script_hash_by_registry_address_fn
      sys_get_script_hash_by_registry_address;
  gw_snapshot_fn sys_snapshot;
  gw_revert_fn sys_revert;
  _gw_load_raw_fn _internal_load_raw;
  _gw_store_raw_fn _internal_store_raw;
} gw_context_t;

#include "common.h"

int _internal_load_raw(gw_context_t *ctx, const uint8_t raw_key[GW_VALUE_BYTES],
                       uint8_t value[GW_VALUE_BYTES]) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  int ret = syscall(GW_SYS_LOAD, raw_key, value, 0, 0, 0, 0);
  if (ret != 0) {
    printf("failed internal_load_raw");
    /* Even we load via syscall, the data structure in the bottom is a SMT */
    return GW_FATAL_SMT_FETCH;
  }
  return 0;
}

int _internal_store_raw(gw_context_t *ctx, const uint8_t raw_key[GW_KEY_BYTES],
                        const uint8_t value[GW_VALUE_BYTES]) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  int ret = syscall(GW_SYS_STORE, raw_key, value, 0, 0, 0, 0);
  if (ret != 0) {
    printf("failed internal_store_raw");
    /* Even we load via syscall, the data structure in the bottom is a SMT */
    return GW_FATAL_SMT_STORE;
  }
  return 0;
}

int sys_load(gw_context_t *ctx, uint32_t account_id, const uint8_t *key,
             const uint64_t key_len, uint8_t value[GW_VALUE_BYTES]) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  int ret = _ensure_account_exists(ctx, account_id);
  if (ret != 0) {
    return ret;
  }

  uint8_t raw_key[GW_KEY_BYTES] = {0};
  gw_build_account_key(account_id, key, key_len, raw_key);
  return _internal_load_raw(ctx, raw_key, value);
}
int sys_store(gw_context_t *ctx, uint32_t account_id, const uint8_t *key,
              const uint64_t key_len, const uint8_t value[GW_VALUE_BYTES]) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  int ret = _ensure_account_exists(ctx, account_id);
  if (ret != 0) {
    return ret;
  }

  uint8_t raw_key[GW_KEY_BYTES];
  gw_build_account_key(account_id, key, key_len, raw_key);
  return _internal_store_raw(ctx, raw_key, value);
}

int sys_get_account_nonce(gw_context_t *ctx, uint32_t account_id,
                          uint32_t *nonce) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  int ret = _ensure_account_exists(ctx, account_id);
  if (ret != 0) {
    return ret;
  }

  uint8_t key[32] = {0};
  gw_build_account_field_key(account_id, GW_ACCOUNT_NONCE, key);
  uint8_t value[32] = {0};
  ret = _internal_load_raw(ctx, key, value);
  if (ret != 0) {
    return ret;
  }
  _gw_fast_memcpy(nonce, value, sizeof(uint32_t));
  return 0;
}

/* set call return data */
int sys_set_program_return_data(gw_context_t *ctx, uint8_t *data,
                                uint64_t len) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  if (len > GW_MAX_RETURN_DATA_SIZE) {
    printf("Exceeded max return data size");
    return GW_FATAL_BUFFER_OVERFLOW;
  }
  return syscall(GW_SYS_SET_RETURN_DATA, data, len, 0, 0, 0, 0);
}

/* Get account id by account script_hash */
int sys_get_account_id_by_script_hash(gw_context_t *ctx,
                                      uint8_t script_hash[32],
                                      uint32_t *account_id) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  uint8_t raw_key[32] = {0};
  uint8_t value[32] = {0};
  gw_build_script_hash_to_account_id_key(script_hash, raw_key);
  int ret = _internal_load_raw(ctx, raw_key, value);
  if (ret != 0) {
    return ret;
  }
  *account_id = *((uint32_t *)value);
  /* check exists flag */
  int exists = value[4] == 1;
  if (exists) {
    return 0;
  }

  return GW_ERROR_ACCOUNT_NOT_EXISTS;
}

/* Get account script_hash by account id */
int sys_get_script_hash_by_account_id(gw_context_t *ctx, uint32_t account_id,
                                      uint8_t script_hash[32]) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  int ret = _ensure_account_exists(ctx, account_id);
  if (ret != 0) {
    return ret;
  }

  uint8_t raw_key[32] = {0};
  gw_build_account_field_key(account_id, GW_ACCOUNT_SCRIPT_HASH, raw_key);
  return _internal_load_raw(ctx, raw_key, script_hash);
}

/* Get account script by account id */
int sys_get_account_script(gw_context_t *ctx, uint32_t account_id,
                           uint64_t *len, uint64_t offset, uint8_t *script) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  /* get account script hash */
  int ret;
  uint8_t script_hash[32] = {0};
  ret = sys_get_script_hash_by_account_id(ctx, account_id, script_hash);
  if (ret != 0) {
    return ret;
  }

  if (_is_zero_hash(script_hash)) {
    printf("account script_hash is zero, which means account isn't exist");
    return GW_ERROR_NOT_FOUND;
  }

  volatile uint64_t inner_len = *len;
  ret = syscall(GW_SYS_LOAD_ACCOUNT_SCRIPT, script, &inner_len, offset,
                account_id, 0, 0);
  *len = inner_len;
  return ret;
}
/* Store data by data hash */
int sys_store_data(gw_context_t *ctx, uint64_t data_len, uint8_t *data) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  if (0 == data_len) {
    return 0;
  }
  if (data_len > GW_MAX_DATA_SIZE) {
    printf("Exceeded max store data size");
    return GW_FATAL_INVALID_DATA;
  }
  return syscall(GW_SYS_STORE_DATA, data_len, data, 0, 0, 0, 0);
}
/* Load data by data hash */
int sys_load_data(gw_context_t *ctx, uint8_t data_hash[32], uint64_t *len,
                  uint64_t offset, uint8_t *data) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  if (0 == *len) {
    return 0;
  }

  /* Check data_hash_key */
  int data_exists = 0;
  int ret = _check_data_hash_exist(ctx, data_hash, &data_exists);
  if (ret != 0) {
    return ret;
  }

  if (!data_exists) {
    printf("data hash not exist");
    /* return not found if data isn't exist in the state tree */
    return GW_ERROR_NOT_FOUND;
  }

  volatile uint64_t inner_len = *len;
  ret = syscall(GW_SYS_LOAD_DATA, data, &inner_len, offset, data_hash, 0, 0);
  *len = inner_len;
  return ret;
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

int sys_get_block_hash(gw_context_t *ctx, uint64_t number,
                       uint8_t block_hash[32]) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  return syscall(GW_SYS_GET_BLOCK_HASH, block_hash, number, 0, 0, 0, 0);
}

int sys_create(gw_context_t *ctx, uint8_t *script, uint64_t script_len,
               uint32_t *account_id) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  if (script_len > GW_MAX_SCRIPT_SIZE) {
    return GW_ERROR_INVALID_ACCOUNT_SCRIPT;
  }

  /* calculate script_hash */
  uint8_t script_hash[32] = {0};
  blake2b_state blake2b_ctx;
  blake2b_init(&blake2b_ctx, 32);
  blake2b_update(&blake2b_ctx, script, script_len);
  blake2b_final(&blake2b_ctx, script_hash, 32);

  /* check existence */
  int account_exist = 0;
  int ret =
      _check_account_exists_by_script_hash(ctx, script_hash, &account_exist);
  if (ret != 0) {
    return ret;
  }
  if (account_exist) {
    return GW_ERROR_DUPLICATED_SCRIPT_HASH;
  }

  return syscall(GW_SYS_CREATE, script, script_len, account_id, 0, 0, 0);
}

int sys_recover_account(struct gw_context_t *ctx, uint8_t message[32],
                        uint8_t *signature, uint64_t signature_len,
                        uint8_t code_hash[32], uint8_t *script,
                        uint64_t *script_len) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  volatile uint64_t inner_script_len = 0;
  int ret = syscall(GW_SYS_RECOVER_ACCOUNT, script, &inner_script_len, message,
                    signature, signature_len, code_hash);

  if (0 == ret && *script_len < inner_script_len) {
    printf("recover account: buffer overflow");
    return GW_FATAL_BUFFER_OVERFLOW;
  }

  *script_len = inner_script_len;
  return ret;
}

int sys_bn_add(const uint8_t *input, const size_t input_size, uint8_t *output) {
  volatile uint64_t output_len = 64;
  return syscall(GW_SYS_BN_ADD, output, &output_len, 0, input, input_size, 0);
}

int sys_bn_mul(const uint8_t *input, const size_t input_size, uint8_t *output) {
  volatile uint64_t output_len = 64;
  return syscall(GW_SYS_BN_MUL, output, &output_len, 0, input, input_size, 0);
}

int sys_bn_pairing(const uint8_t *input, const size_t input_size,
                   uint8_t *output) {
  volatile uint64_t output_size = 32;
  return syscall(GW_SYS_BN_PAIRING, output, &output_size, 0 /* offset = 0 */,
                 input, input_size, 0);
}

int sys_log(gw_context_t *ctx, uint32_t account_id, uint8_t service_flag,
            uint64_t data_length, const uint8_t *data) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  int ret = _ensure_account_exists(ctx, account_id);
  if (ret != 0) {
    return ret;
  }

  return syscall(GW_SYS_LOG, account_id, service_flag, data_length, data, 0, 0);
}

int sys_pay_fee(gw_context_t *ctx, gw_reg_addr_t addr, uint32_t sudt_id,
                uint256_t amount) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  int ret = _ensure_account_exists(ctx, sudt_id);
  if (ret != 0) {
    return ret;
  }
  uint8_t buf[32] = {0};
  int len = GW_REG_ADDR_SIZE(addr);
  if (len > 32) {
    printf(
        "sys_pay_fee: invalid addr len, "
        "expect <= 20");
    return GW_FATAL_BUFFER_OVERFLOW;
  }
  _gw_cpy_addr(buf, addr);

  return syscall(GW_SYS_PAY_FEE, buf, len, sudt_id, (uint8_t *)&amount, 0, 0);
}

int sys_snapshot(gw_context_t *ctx, uint32_t *snapshot) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  return syscall(GW_SYS_SNAPSHOT, snapshot, 0, 0, 0, 0, 0);
}

int sys_revert(gw_context_t *ctx, uint32_t snapshot) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  return syscall(GW_SYS_REVERT, snapshot, 0, 0, 0, 0, 0);
}

int _sys_load_rollup_config(uint8_t *addr, uint64_t *len) {
  volatile uint64_t inner_len = *len;
  int ret = syscall(GW_SYS_LOAD_ROLLUP_CONFIG, addr, &inner_len, 0, 0, 0, 0);
  *len = inner_len;

  if (*len > GW_MAX_ROLLUP_CONFIG_SIZE) {
    printf("length too long");
    return GW_FATAL_INVALID_DATA;
  }
  mol_seg_t config_seg;
  config_seg.ptr = addr;
  config_seg.size = *len;
  if (MolReader_RollupConfig_verify(&config_seg, false) != MOL_OK) {
    printf("rollup config cell data is not RollupConfig format");
    return GW_FATAL_INVALID_DATA;
  }

  return ret;
}

int gw_context_init(gw_context_t *ctx) {
  /* setup syscalls */
  ctx->sys_load = sys_load;
  ctx->sys_store = sys_store;
  ctx->sys_set_program_return_data = sys_set_program_return_data;
  ctx->sys_create = sys_create;
  ctx->sys_get_account_id_by_script_hash = sys_get_account_id_by_script_hash;
  ctx->sys_get_script_hash_by_account_id = sys_get_script_hash_by_account_id;
  ctx->sys_get_account_nonce = sys_get_account_nonce;
  ctx->sys_get_account_script = sys_get_account_script;
  ctx->sys_store_data = sys_store_data;
  ctx->sys_load_data = sys_load_data;
  ctx->sys_get_block_hash = sys_get_block_hash;
  ctx->sys_recover_account = sys_recover_account;
  ctx->sys_bn_add = sys_bn_add;
  ctx->sys_bn_mul = sys_bn_mul;
  ctx->sys_bn_pairing = sys_bn_pairing;
  ctx->sys_pay_fee = sys_pay_fee;
  ctx->sys_log = sys_log;
  ctx->sys_get_registry_address_by_script_hash =
      _gw_get_registry_address_by_script_hash;
  ctx->sys_get_script_hash_by_registry_address =
      _gw_get_script_hash_by_registry_address;
  ctx->sys_snapshot = sys_snapshot;
  ctx->sys_revert = sys_revert;
  ctx->_internal_load_raw = _internal_load_raw;
  ctx->_internal_store_raw = _internal_store_raw;

  /* initialize context */
  uint8_t tx_buf[GW_MAX_L2TX_SIZE];
  uint64_t len = GW_MAX_L2TX_SIZE;
  int ret = _sys_load_l2transaction(tx_buf, &len);
  if (ret != 0) {
    return ret;
  }
  if (len > GW_MAX_L2TX_SIZE) {
    return GW_FATAL_INVALID_DATA;
  }

  mol_seg_t l2transaction_seg;
  l2transaction_seg.ptr = tx_buf;
  l2transaction_seg.size = len;
  ret = gw_parse_transaction_context(&ctx->transaction_context,
                                     &l2transaction_seg);
  if (ret != 0) {
    return ret;
  }

  uint8_t block_info_buf[GW_MAX_BLOCK_INFO_SIZE] = {0};
  len = GW_MAX_BLOCK_INFO_SIZE;
  ret = _sys_load_block_info(block_info_buf, &len);
  if (ret != 0) {
    return ret;
  }

  mol_seg_t block_info_seg;
  block_info_seg.ptr = block_info_buf;
  block_info_seg.size = len;
  ret = gw_parse_block_info(&ctx->block_info, &block_info_seg);
  if (ret != 0) {
    return ret;
  }

  ctx->rollup_config_size = GW_MAX_ROLLUP_CONFIG_SIZE;
  ret = _sys_load_rollup_config(ctx->rollup_config, &ctx->rollup_config_size);
  if (ret != 0) {
    return ret;
  }

  /* init original sender nonce */
  ret = _load_sender_nonce(ctx, &ctx->original_sender_nonce);
  if (ret != 0) {
    printf("failed to init original sender nonce");
    return ret;
  }

  return 0;
}

int gw_finalize(gw_context_t *ctx) {
  /* update sender nonce */
  int ret = _increase_sender_nonce(ctx);
  if (ret != 0) {
    printf("failed to update original sender nonce");
    return ret;
  }

  return 0;
}

int gw_verify_sudt_account(gw_context_t *ctx, uint32_t sudt_id) {
  uint8_t script_buffer[GW_MAX_SCRIPT_SIZE];
  uint64_t script_len = GW_MAX_SCRIPT_SIZE;
  int ret = sys_get_account_script(ctx, sudt_id, &script_len, 0, script_buffer);
  if (ret != 0) {
    return ret;
  }
  if (script_len > GW_MAX_SCRIPT_SIZE) {
    return GW_FATAL_INVALID_SUDT_SCRIPT;
  }
  mol_seg_t script_seg;
  script_seg.ptr = script_buffer;
  script_seg.size = script_len;
  if (MolReader_Script_verify(&script_seg, false) != MOL_OK) {
    printf("load account script: invalid script");
    return GW_FATAL_INVALID_SUDT_SCRIPT;
  }
  mol_seg_t code_hash_seg = MolReader_Script_get_code_hash(&script_seg);
  mol_seg_t hash_type_seg = MolReader_Script_get_hash_type(&script_seg);

  mol_seg_t rollup_config_seg;
  rollup_config_seg.ptr = ctx->rollup_config;
  rollup_config_seg.size = ctx->rollup_config_size;
  mol_seg_t l2_sudt_validator_script_type_hash =
      MolReader_RollupConfig_get_l2_sudt_validator_script_type_hash(
          &rollup_config_seg);
  if (memcmp(l2_sudt_validator_script_type_hash.ptr, code_hash_seg.ptr, 32) !=
      0) {
    return GW_FATAL_INVALID_SUDT_SCRIPT;
  }
  if (*hash_type_seg.ptr != 1) {
    return GW_FATAL_INVALID_SUDT_SCRIPT;
  }
  return 0;
}
#endif
