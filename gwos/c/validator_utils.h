#ifndef GW_VALIDATOR_H_
#define GW_VALIDATOR_H_

#include "blake2b.h"
#include "blockchain.h"
#include "ckb_smt.h"
#include "ckb_syscalls.h"
#include "gw_def.h"
#include "uint256.h"

#define SCRIPT_HASH_TYPE_DATA 0
#define SCRIPT_HASH_TYPE_TYPE 1
#define TARGET_TYPE_TRANSACTION 0

/* buffer size */
#define GW_MAX_KV_PROOF_SIZE 32768
#define GW_MAX_CHALLENGE_LOCK_SCRIPT_SIZE 4096
#define GW_MAX_GET_BLOCK_HASH_DEPTH 256

/* functions */
int _gw_check_account_script_is_allowed(uint8_t rollup_script_hash[32],
                                        mol_seg_t *script_seg,
                                        mol_seg_t *rollup_config_seg);
void _gw_block_smt_key(uint8_t key[32], uint64_t number);

typedef struct {
  uint8_t merkle_root[32];
  uint32_t count;
} gw_account_merkle_state_t;

/* The struct is design for lazy get_account_script by account id */
typedef struct {
  uint8_t hash[32];
  uint8_t script[GW_MAX_SCRIPT_SIZE];
  uint32_t script_len;
} gw_script_entry_t;

/* The struct is design for lazy sys_load_data*/
typedef struct {
  uint8_t hash[32];
  uint8_t *data;
  uint32_t data_len;
} gw_load_data_entry_t;

/* Call receipt */
typedef struct {
  uint8_t return_data[GW_MAX_RETURN_DATA_SIZE];
  uint32_t return_data_len;
} gw_call_receipt_t;

typedef struct gw_context_t {
  /* verification context */
  gw_transaction_context_t transaction_context;
  gw_block_info_t block_info;
  uint8_t rollup_config[GW_MAX_ROLLUP_CONFIG_SIZE];
  size_t rollup_config_size;
  uint8_t rollup_script_hash[32];

  /* layer2 syscalls */
  gw_load_fn sys_load;
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
  gw_recover_account_fn sys_recover_account;
  gw_log_fn sys_log;
  gw_pay_fee_fn sys_pay_fee;
  gw_bn_add sys_bn_add;
  gw_bn_mul sys_bn_mul;
  gw_bn_pairing sys_bn_pairing;
  gw_get_registry_address_by_script_hash_fn
      sys_get_registry_address_by_script_hash;
  gw_get_script_hash_by_registry_address_fn
      sys_get_script_hash_by_registry_address;
  gw_snapshot_fn sys_snapshot;
  gw_revert_fn sys_revert;
  gw_check_sudt_addr_permission_fn sys_check_sudt_addr_permission;
  _gw_load_raw_fn _internal_load_raw;
  _gw_store_raw_fn _internal_store_raw;

  /* validator specific context */
  gw_account_merkle_state_t prev_account; /* RawL2Block.prev_account */
  gw_account_merkle_state_t post_account; /* RawL2Block.post_account */

  /* challenged tx index */
  uint32_t tx_index;

  /* sender's original nonce */
  uint32_t original_sender_nonce;

  /* tx check point */
  uint8_t prev_tx_checkpoint[32];
  uint8_t post_tx_checkpoint[32];

  /* kv state */
  smt_state_t kv_state;
  smt_pair_t kv_pairs[GW_MAX_KV_PAIRS];

  /* block hashes */
  smt_state_t block_hashes_state;
  smt_pair_t block_hashes_pairs[GW_MAX_GET_BLOCK_HASH_DEPTH];

  /* SMT proof */
  uint8_t kv_state_proof[GW_MAX_KV_PROOF_SIZE];
  size_t kv_state_proof_size;

  /* account count */
  uint32_t account_count;

  /* All the scripts account has read and write */
  gw_script_entry_t scripts[GW_MAX_SCRIPT_ENTRIES_SIZE];
  size_t script_entries_size;

  /* All the data load */
  gw_load_data_entry_t load_data[GW_MAX_LOAD_DATA_ENTRIES_SIZE];
  size_t load_data_entries_size;

  /* return data hash */
  uint8_t return_data_hash[32];
  gw_call_receipt_t receipt;
} gw_context_t;

#include "common.h"

int _internal_load_raw(gw_context_t *ctx, const uint8_t raw_key[GW_VALUE_BYTES],
                       uint8_t value[GW_VALUE_BYTES]) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  int ret = smt_state_fetch(&ctx->kv_state, raw_key, value);
  if (ret != 0) {
    printf("failed internal_load_raw");
    return GW_FATAL_SMT_FETCH;
  }
  return 0;
}

int _internal_store_raw(gw_context_t *ctx, const uint8_t raw_key[GW_KEY_BYTES],
                        const uint8_t value[GW_VALUE_BYTES]) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  int ret = smt_state_insert(&ctx->kv_state, raw_key, value);
  if (ret != 0) {
    printf("failed internal_store_raw");
    return GW_FATAL_SMT_STORE;
  }
  return 0;
}

int sys_load(gw_context_t *ctx, uint32_t account_id, const uint8_t *key,
             const size_t key_len, uint8_t value[GW_VALUE_BYTES]) {
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
              const size_t key_len, const uint8_t value[GW_VALUE_BYTES]) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  int ret = _ensure_account_exists(ctx, account_id);
  if (ret != 0) {
    return ret;
  }

  uint8_t raw_key[GW_KEY_BYTES] = {0};
  gw_build_account_key(account_id, key, key_len, raw_key);
  return _internal_store_raw(ctx, raw_key, value);
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
  _gw_fast_memcpy(ctx->receipt.return_data, data, len);
  ctx->receipt.return_data_len = len;
  return 0;
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

/* Get nonce by account id */
int sys_get_account_nonce(gw_context_t *ctx, uint32_t account_id,
                          uint32_t *nonce) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  int ret = _ensure_account_exists(ctx, account_id);
  if (ret != 0) {
    return ret;
  }

  uint8_t raw_key[32] = {0};
  gw_build_account_field_key(account_id, GW_ACCOUNT_NONCE, raw_key);
  uint8_t value[32] = {0};
  ret = smt_state_fetch(&ctx->kv_state, raw_key, value);
  if (ret != 0) {
    printf("sys_get_account_nonce, failed to load smt, ret: %d", ret);
    return GW_FATAL_SMT_FETCH;
  }
  _gw_fast_memcpy(nonce, value, sizeof(uint32_t));
  return 0;
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
    return GW_ERROR_ACCOUNT_NOT_EXISTS;
  }

  /* iterate all scripts to find account's script */
  gw_script_entry_t *entry = NULL;
  for (uint32_t i = 0; i < ctx->script_entries_size; i++) {
    gw_script_entry_t *current = &ctx->scripts[i];
    if (memcmp(current->hash, script_hash, 32) == 0) {
      entry = current;
      break;
    }
  }

  if (entry == NULL) {
    printf(
        "account script_hash exist, but we can't found, we miss the "
        "necessary context");
    return GW_FATAL_SCRIPT_NOT_FOUND;
  }

  /* return account script */
  size_t new_len;
  size_t data_len = entry->script_len;
  if (offset >= data_len) {
    printf("account script offset is bigger than actual script len");
    new_len = 0;
  } else if ((offset + *len) > data_len) {
    new_len = data_len - offset;
  } else {
    new_len = *len;
  }
  if (new_len > 0) {
    _gw_fast_memcpy(script, entry->script + offset, new_len);
  }
  *len = new_len;
  return 0;
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
  /* In validator, we do not need to actually store data.
     We only need to update the data_hash in the state tree
   */

  /* Compute data_hash */
  uint8_t data_hash[GW_KEY_BYTES] = {0};
  blake2b_state blake2b_ctx;
  blake2b_init(&blake2b_ctx, GW_KEY_BYTES);
  blake2b_update(&blake2b_ctx, data, data_len);
  blake2b_final(&blake2b_ctx, data_hash, GW_KEY_BYTES);

  /* Compute data_hash_key */
  uint8_t raw_key[GW_KEY_BYTES] = {0};
  gw_build_data_hash_key(data_hash, raw_key);

  /* value */
  uint32_t one = 1;
  uint8_t value[GW_VALUE_BYTES] = {0};
  _gw_fast_memcpy(value, &one, sizeof(uint32_t));

  /* update state */
  return _internal_store_raw(ctx, raw_key, value);
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
    /* return not found if data isn't exist in the state tree */
    return GW_ERROR_NOT_FOUND;
  }

  /* Try load data from witness */
  gw_load_data_entry_t *entry = NULL;
  for (uint32_t i = 0; i < ctx->load_data_entries_size; i++) {
    gw_load_data_entry_t *current = &ctx->load_data[i];
    if (memcmp(current->hash, data_hash, 32) == 0) {
      entry = current;
      break;
    }
  }

  if (NULL != entry) {
    size_t new_len;
    size_t data_len = entry->data_len;
    if (offset >= data_len) {
      printf("load data offset is bigger than actual data len");
      new_len = 0;
    } else if ((offset + *len) > data_len) {
      new_len = data_len - offset;
    } else {
      new_len = *len;
    }
    if (new_len > 0) {
      _gw_fast_memcpy(data, entry->data + offset, new_len);
    }
    *len = new_len;
    return 0;
  }

  size_t index = 0;
  uint64_t hash_len = 32;
  uint8_t hash[32] = {0};

  /* iterate all dep cells in loop */
  while (1) {
    ret = ckb_load_cell_by_field(hash, &hash_len, 0, index, CKB_SOURCE_CELL_DEP,
                                 CKB_CELL_FIELD_DATA_HASH);
    if (ret == CKB_SUCCESS) {
      /* check data hash */
      if (memcmp(hash, data_hash, 32) == 0) {
        uint64_t data_len = (uint64_t)*len;
        ret = ckb_load_cell_data(data, &data_len, offset, index,
                                 CKB_SOURCE_CELL_DEP);
        if (ret != CKB_SUCCESS) {
          printf("load cell data failed");
          return GW_FATAL_DATA_CELL_NOT_FOUND;
        }
        *len = (uint32_t)data_len;
        return 0;
      }
    } else if (ret == CKB_ITEM_MISSING) {
      printf("not found cell data by data hash");
      return GW_FATAL_DATA_CELL_NOT_FOUND;
    } else {
      printf("load cell data hash failed");
      return GW_FATAL_DATA_CELL_NOT_FOUND;
    }
    index += 1;
  }
  /* dead code */
  printf("can't find data cell");
  return GW_FATAL_INVALID_CONTEXT;
}

int sys_get_block_hash(gw_context_t *ctx, uint64_t number,
                       uint8_t block_hash[32]) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  uint8_t key[32] = {0};
  _gw_block_smt_key(key, number);
  int ret = smt_state_fetch(&ctx->block_hashes_state, key, block_hash);
  if (ret != 0) {
    printf("sys_get_block_hash: failed to load smt, ret: %d", ret);
    return GW_FATAL_SMT_FETCH;
  }
  return 0;
}

int sys_recover_account(gw_context_t *ctx, uint8_t message[32],
                        uint8_t *signature, uint64_t signature_len,
                        uint8_t code_hash[32], uint8_t *script,
                        uint64_t *script_len) {
  /* iterate all inputs */
  uint8_t lock_script[GW_MAX_SCRIPT_SIZE];
  uint64_t len = 0;
  uint64_t ret = 0;
  int i;
  for (i = 0; true; i++) {
    len = GW_MAX_SCRIPT_SIZE;
    /* load input's lock */
    ret = ckb_checked_load_cell_by_field(lock_script, &len, 0, i,
                                         CKB_SOURCE_INPUT, CKB_CELL_FIELD_LOCK);
    if (ret != 0) {
      printf("sys_recover_account: failed to load input's lock");
      return GW_FATAL_INVALID_CONTEXT;
    }
    /* convert to molecule */
    mol_seg_t script_seg;
    script_seg.ptr = lock_script;
    script_seg.size = len;
    if (MolReader_Script_verify(&script_seg, false) != MOL_OK) {
      return GW_FATAL_INVALID_DATA;
    }
    /* check lock's code_hash & hash_type */
    mol_seg_t code_hash_seg = MolReader_Script_get_code_hash(&script_seg);
    if (memcmp(code_hash, code_hash_seg.ptr, 32) != 0) {
      continue;
    }
    mol_seg_t hash_type_seg = MolReader_Script_get_hash_type(&script_seg);
    if ((*(uint8_t *)hash_type_seg.ptr) != SCRIPT_HASH_TYPE_TYPE) {
      continue;
    }
    /* load message from cell.data[33..65] */
    uint8_t checked_message[32];
    len = 32;
    ret = ckb_load_cell_data(checked_message, &len, 33, i, CKB_SOURCE_INPUT);
    if (ret != 0) {
      printf("recover account: failed to load cell data");
      continue;
    }
    if (len != 32) {
      printf("recover account: invalid data format");
      continue;
    }
    /* check message */
    if (memcmp(message, checked_message, 32) != 0) {
      continue;
    }
    /* load signature */
    uint8_t witness[GW_MAX_WITNESS_SIZE];
    len = GW_MAX_WITNESS_SIZE;
    ret = ckb_checked_load_witness(witness, &len, 0, i, CKB_SOURCE_INPUT);
    if (ret != 0) {
      printf("recover account: failed to load witness");
      continue;
    }
    mol_seg_t witness_args_seg;
    witness_args_seg.ptr = witness;
    witness_args_seg.size = len;
    if (MolReader_WitnessArgs_verify(&witness_args_seg, false) != MOL_OK) {
      printf("recover account: invalid witness args");
      continue;
    }
    mol_seg_t witness_lock_seg =
        MolReader_WitnessArgs_get_lock(&witness_args_seg);
    if (MolReader_BytesOpt_is_none(&witness_lock_seg)) {
      printf("recover account: witness args has no lock field");
      continue;
    }
    mol_seg_t signature_seg = MolReader_Bytes_raw_bytes(&witness_lock_seg);

    /* check signature */
    if (signature_len != signature_seg.size) {
      continue;
    }
    if (memcmp(signature, signature_seg.ptr, signature_len) != 0) {
      continue;
    }

    /* found script, recover account script */
    if (*script_len < script_seg.size) {
      printf("recover account: buffer overflow");
      return GW_FATAL_BUFFER_OVERFLOW;
    }
    _gw_fast_memcpy(script, script_seg.ptr, script_seg.size);
    *script_len = script_seg.size;
    return 0;
  }
  /* Can't found account signature lock from inputs */
  printf(
      "recover account: can't found account signature lock "
      "from inputs");
  return GW_FATAL_SIGNATURE_CELL_NOT_FOUND;
}

int sys_bn_add(const uint8_t *input, const size_t input_size, uint8_t *output) {
  // TODO
  return GW_UNIMPLEMENTED;
}

int sys_bn_mul(const uint8_t *input, const size_t input_size, uint8_t *output) {
  // TODO
  return GW_UNIMPLEMENTED;
}

int sys_bn_pairing(const uint8_t *input, const size_t input_size,
                   uint8_t *output) {
  // TODO
  return GW_UNIMPLEMENTED;
}

int sys_create(gw_context_t *ctx, uint8_t *script, uint64_t script_len,
               uint32_t *account_id) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  /* return failure if scripts slots is full */
  if (ctx->script_entries_size >= GW_MAX_SCRIPT_ENTRIES_SIZE) {
    printf("[sys_create] script slots is full");
    return GW_FATAL_BUFFER_OVERFLOW;
  }

  int ret;
  uint32_t id = ctx->account_count;

  mol_seg_t account_script_seg;
  account_script_seg.ptr = script;
  account_script_seg.size = script_len;
  /* check script */
  mol_seg_t rollup_config_seg;
  rollup_config_seg.ptr = ctx->rollup_config;
  rollup_config_seg.size = ctx->rollup_config_size;
  ret = _gw_check_account_script_is_allowed(
      ctx->rollup_script_hash, &account_script_seg, &rollup_config_seg);
  if (ret != 0) {
    printf("[sys_create] reject invalid account script");
    return ret;
  }

  /* calculate script_hash */
  uint8_t script_hash[32] = {0};
  blake2b_state blake2b_ctx;
  blake2b_init(&blake2b_ctx, 32);
  blake2b_update(&blake2b_ctx, script, script_len);
  blake2b_final(&blake2b_ctx, script_hash, 32);

  /* check existence */
  int account_exist = 0;
  ret = _check_account_exists_by_script_hash(ctx, script_hash, &account_exist);
  if (ret != 0) {
    return ret;
  }
  if (account_exist) {
    return GW_ERROR_DUPLICATED_SCRIPT_HASH;
  }

  /* init account nonce */
  uint8_t nonce_key[32] = {0};
  uint8_t nonce_value[32] = {0};
  gw_build_account_field_key(id, GW_ACCOUNT_NONCE, nonce_key);
  ret = _internal_store_raw(ctx, nonce_key, nonce_value);
  if (ret != 0) {
    return ret;
  }

  /* init account script hash */
  uint8_t script_hash_key[32] = {0};
  gw_build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH, script_hash_key);
  ret = _internal_store_raw(ctx, script_hash_key, script_hash);
  if (ret != 0) {
    return ret;
  }

  /* init script hash -> account_id */
  uint8_t script_hash_to_id_key[32] = {0};
  uint8_t script_hash_to_id_value[32] = {0};
  gw_build_script_hash_to_account_id_key(script_hash, script_hash_to_id_key);
  /* set id */
  _gw_fast_memcpy(script_hash_to_id_value, (uint8_t *)(&id), 4);
  /* set exists flag */
  script_hash_to_id_value[4] = 1;
  ret =
      _internal_store_raw(ctx, script_hash_to_id_key, script_hash_to_id_value);
  if (ret != 0) {
    return ret;
  }

  /* build script entry */
  gw_script_entry_t script_entry = {0};
  /* copy script to entry's buf */
  _gw_fast_memcpy(&script_entry.script, account_script_seg.ptr,
                  account_script_seg.size);
  script_entry.script_len = account_script_seg.size;
  /* set script hash */
  _gw_fast_memcpy(&script_entry.hash, script_hash, 32);

  /* insert script entry to ctx */
  _gw_fast_memcpy(&ctx->scripts[ctx->script_entries_size], &script_entry,
                  sizeof(gw_script_entry_t));
  ctx->script_entries_size += 1;
  ctx->account_count += 1;
  *account_id = id;

  return 0;
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
  /* do nothing */
  return 0;
}

int sys_pay_fee(gw_context_t *ctx, gw_reg_addr_t payer_addr, uint32_t sudt_id,
                uint256_t amount) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  int ret = _ensure_account_exists(ctx, sudt_id);
  if (ret != 0) {
    return ret;
  }

  /* do nothing */
  return 0;
}

int sys_snapshot(gw_context_t *ctx, uint32_t *snapshot) {
  return GW_UNIMPLEMENTED;
}

int sys_revert(gw_context_t *ctx, uint32_t snapshot) {
  return GW_UNIMPLEMENTED;
}

int sys_check_sudt_addr_permission(gw_context_t *ctx,
                                   const uint8_t sudt_proxy_addr[20]) {
  return GW_UNIMPLEMENTED;
}

/* Find cell by type hash */
int _find_cell_by_type_hash(uint8_t type_hash[32], uint64_t source,
                            uint64_t *index) {
  uint8_t buf[32] = {0};
  uint64_t buf_len = 32;
  *index = 0;
  while (1) {
    int ret = ckb_checked_load_cell_by_field(buf, &buf_len, 0, *index, source,
                                             CKB_CELL_FIELD_TYPE_HASH);
    if (ret == CKB_INDEX_OUT_OF_BOUND) {
      printf(
          "_find_cell_by_type_hash: return not found cell index: %ld, source: "
          "%ld",
          *index, source);
      return GW_ERROR_NOT_FOUND;
    }
    if (ret == CKB_SUCCESS && memcmp(type_hash, buf, 32) == 0) {
      return 0;
    }
    *index += 1;
  }
}

/* Find cell by data hash */
int _find_cell_by_data_hash(uint8_t data_hash[32], uint64_t source,
                            uint64_t *index) {
  uint8_t buf[32] = {0};
  uint64_t buf_len = 32;
  *index = 0;
  while (1) {
    int ret = ckb_checked_load_cell_by_field(buf, &buf_len, 0, *index, source,
                                             CKB_CELL_FIELD_DATA_HASH);
    if (ret == CKB_INDEX_OUT_OF_BOUND) {
      printf("_find_cell_by_data_hash: failed to load cell data hash");
      return GW_FATAL_INVALID_CONTEXT;
    }
    if (ret == CKB_SUCCESS && memcmp(data_hash, buf, 32) == 0) {
      return 0;
    }
    *index += 1;
  }
}

/* load rollup script_hash from current script.args first 32 bytes */
int _load_rollup_script_hash(uint8_t rollup_script_hash[32]) {
  uint8_t script_buf[GW_MAX_SCRIPT_SIZE];
  uint64_t len = GW_MAX_SCRIPT_SIZE;
  int ret = ckb_checked_load_script(script_buf, &len, 0);
  if (ret != 0) {
    printf("_load_rollup_script_hash: failed to load script");
    return GW_FATAL_INVALID_CONTEXT;
  }
  mol_seg_t script_seg;
  script_seg.ptr = script_buf;
  script_seg.size = len;
  if (MolReader_Script_verify(&script_seg, false) != MOL_OK) {
    return GW_FATAL_INVALID_DATA;
  }
  mol_seg_t args_seg = MolReader_Script_get_args(&script_seg);
  mol_seg_t raw_bytes_seg = MolReader_Bytes_raw_bytes(&args_seg);
  if (raw_bytes_seg.size < 32) {
    printf("current script is less than 32 bytes");
    return GW_FATAL_INVALID_DATA;
  }
  _gw_fast_memcpy(rollup_script_hash, raw_bytes_seg.ptr, 32);
  return 0;
}

/* Load config config */
int _load_rollup_config(uint8_t config_cell_data_hash[32],
                        uint8_t rollup_config_buf[GW_MAX_ROLLUP_CONFIG_SIZE],
                        uint64_t *rollup_config_size) {
  /* search rollup config cell from deps */
  uint64_t config_cell_index = 0;
  int ret = _find_cell_by_data_hash(config_cell_data_hash, CKB_SOURCE_CELL_DEP,
                                    &config_cell_index);
  if (ret != 0) {
    printf("failed to find rollup config");
    return ret;
  }
  /* read data from rollup config cell */
  *rollup_config_size = GW_MAX_ROLLUP_CONFIG_SIZE;
  ret = ckb_checked_load_cell_data(rollup_config_buf, rollup_config_size, 0,
                                   config_cell_index, CKB_SOURCE_CELL_DEP);
  if (ret != 0) {
    printf("_load_rollup_config: failed to load data from rollup config cell");
    return GW_FATAL_INVALID_CONTEXT;
  }

  /* verify rollup config */
  mol_seg_t config_seg;
  config_seg.ptr = rollup_config_buf;
  config_seg.size = *rollup_config_size;
  if (MolReader_RollupConfig_verify(&config_seg, false) != MOL_OK) {
    printf("rollup config cell data is not RollupConfig format");
    return GW_FATAL_INVALID_DATA;
  }

  return 0;
}

/* Load challenge cell lock args */
int _load_challenge_lock_args(
    uint8_t rollup_script_hash[32], uint8_t challenge_script_type_hash[32],
    uint8_t challenge_script_buf[GW_MAX_CHALLENGE_LOCK_SCRIPT_SIZE],
    uint64_t source, uint64_t *index, mol_seg_t *lock_args) {
  uint64_t len;
  *index = 0;
  while (1) {
    /* load challenge lock script */
    len = GW_MAX_CHALLENGE_LOCK_SCRIPT_SIZE;
    int ret = ckb_checked_load_cell_by_field(
        challenge_script_buf, &len, 0, *index, source, CKB_CELL_FIELD_LOCK);
    if (ret != CKB_SUCCESS) {
      printf("_load_challenge_lock_args failed to load cell lock");
      return GW_FATAL_INVALID_CONTEXT;
    }
    mol_seg_t script_seg;
    script_seg.ptr = challenge_script_buf;
    script_seg.size = len;
    if (MolReader_Script_verify(&script_seg, false) != MOL_OK) {
      return GW_FATAL_INVALID_DATA;
    }

    /* check code_hash & hash type */
    mol_seg_t code_hash_seg = MolReader_Script_get_code_hash(&script_seg);
    mol_seg_t hash_type_seg = MolReader_Script_get_hash_type(&script_seg);
    if (memcmp(code_hash_seg.ptr, challenge_script_type_hash, 32) == 0 &&
        *(uint8_t *)hash_type_seg.ptr == SCRIPT_HASH_TYPE_TYPE) {
      mol_seg_t args_seg = MolReader_Script_get_args(&script_seg);
      mol_seg_t raw_args_seg = MolReader_Bytes_raw_bytes(&args_seg);

      /* challenge lock script must start with a 32 bytes rollup script hash */
      if (raw_args_seg.size < 32) {
        printf("challenge lock script's args is less than 32 bytes");
        return GW_FATAL_INVALID_DATA;
      }
      if (memcmp(rollup_script_hash, raw_args_seg.ptr, 32) != 0) {
        printf("challenge lock script's rollup_script_hash mismatch");
        return GW_FATAL_INVALID_DATA;
      }

      /* the remain bytes of args is challenge lock args */
      lock_args->ptr = raw_args_seg.ptr + 32;
      lock_args->size = raw_args_seg.size - 32;
      if (MolReader_ChallengeLockArgs_verify(lock_args, false) != MOL_OK) {
        printf("invalid ChallengeLockArgs");
        return GW_FATAL_INVALID_DATA;
      }
      return 0;
    }
    *index += 1;
  }
}

/* Load verification context */
int _load_verification_context(
    uint8_t rollup_script_hash[32], uint64_t rollup_cell_index,
    uint64_t rollup_cell_source, uint64_t *challenge_cell_index,
    uint8_t challenged_block_hash[32], uint8_t block_merkle_root[32],
    uint32_t *tx_index, uint8_t rollup_config[GW_MAX_ROLLUP_CONFIG_SIZE],
    uint64_t *rollup_config_size) {
  /* load global state from rollup cell */
  uint8_t global_state_buf[sizeof(MolDefault_GlobalState)] = {0};
  uint64_t buf_len = sizeof(MolDefault_GlobalState);
  int ret = ckb_checked_load_cell_data(global_state_buf, &buf_len, 0,
                                       rollup_cell_index, rollup_cell_source);
  if (ret != 0) {
    printf("_load_verification_context: failed to load cell data");
    return GW_FATAL_INVALID_CONTEXT;
  }
  mol_seg_t global_state_seg;
  global_state_seg.ptr = global_state_buf;
  global_state_seg.size = buf_len;

  uint8_t rollup_version = 0;
  if (MolReader_GlobalState_verify(&global_state_seg, false) == MOL_OK) {
    rollup_version =
        *(uint8_t *)MolReader_GlobalState_get_version(&global_state_seg).ptr;
  } else {
    if (MolReader_GlobalStateV0_verify(&global_state_seg, false) != MOL_OK) {
      printf("rollup cell data is not GlobalState format");
      return GW_FATAL_INVALID_DATA;
    }
  }

  /* Get block_merkle_root */
  mol_seg_t block_merkle_state_seg;
  if (0 == rollup_version) {
    block_merkle_state_seg =
        MolReader_GlobalStateV0_get_block(&global_state_seg);
  } else {
    block_merkle_state_seg = MolReader_GlobalState_get_block(&global_state_seg);
  }
  mol_seg_t block_merkle_root_seg =
      MolReader_BlockMerkleState_get_merkle_root(&block_merkle_state_seg);
  if (block_merkle_root_seg.size != 32) {
    printf("invalid block merkle root");
    return GW_FATAL_INVALID_DATA;
  }
  _gw_fast_memcpy(block_merkle_root, block_merkle_root_seg.ptr,
                  block_merkle_root_seg.size);

  /* load rollup config cell */
  mol_seg_t rollup_config_hash_seg;
  if (0 == rollup_version) {
    rollup_config_hash_seg =
        MolReader_GlobalStateV0_get_rollup_config_hash(&global_state_seg);
  } else {
    rollup_config_hash_seg =
        MolReader_GlobalState_get_rollup_config_hash(&global_state_seg);
  }
  ret = _load_rollup_config(rollup_config_hash_seg.ptr, rollup_config,
                            rollup_config_size);
  if (ret != 0) {
    printf("failed to load rollup_config_hash");
    return ret;
  }
  mol_seg_t rollup_config_seg;
  rollup_config_seg.ptr = rollup_config;
  rollup_config_seg.size = *rollup_config_size;

  /* load challenge cell */
  mol_seg_t challenge_script_type_hash_seg =
      MolReader_RollupConfig_get_challenge_script_type_hash(&rollup_config_seg);

  uint8_t challenge_script_buf[GW_MAX_SCRIPT_SIZE];
  *challenge_cell_index = 0;
  mol_seg_t lock_args_seg;
  ret = _load_challenge_lock_args(rollup_script_hash,
                                  challenge_script_type_hash_seg.ptr,
                                  challenge_script_buf, CKB_SOURCE_INPUT,
                                  challenge_cell_index, &lock_args_seg);
  if (ret != 0) {
    printf("failed to load challenge lock args");
    return ret;
  }

  /* check challenge target_type */
  mol_seg_t target_seg = MolReader_ChallengeLockArgs_get_target(&lock_args_seg);

  /* get challenged block hash */
  mol_seg_t block_hash_seg =
      MolReader_ChallengeTarget_get_block_hash(&target_seg);
  if (block_hash_seg.size != 32) {
    printf("invalid challenged block hash");
    return GW_FATAL_INVALID_DATA;
  }
  _gw_fast_memcpy(challenged_block_hash, block_hash_seg.ptr,
                  block_hash_seg.size);

  /* check challenge type */
  mol_seg_t target_type_seg =
      MolReader_ChallengeTarget_get_target_type(&target_seg);
  uint8_t target_type = *(uint8_t *)target_type_seg.ptr;
  if (target_type != TARGET_TYPE_TRANSACTION) {
    printf("challenge target type is invalid");
    return GW_FATAL_INVALID_DATA;
  }
  /* get challenged transaction index */
  mol_seg_t tx_index_seg =
      MolReader_ChallengeTarget_get_target_index(&target_seg);
  _gw_fast_memcpy(tx_index, tx_index_seg.ptr, sizeof(uint32_t));
  return 0;
}

int _gw_cbmt_is_left(uint32_t index) { return (index & 1) == 1; }

int _gw_verify_cbmt_tx_proof(mol_seg_t *proof_seg, mol_seg_t *root_seg,
                             uint32_t tx_index, mol_seg_t *l2tx_seg) {
  mol_seg_t indices_seg = MolReader_CKBMerkleProof_get_indices(proof_seg);
  uint32_t indices_size = MolReader_Uint32Vec_length(&indices_seg);
  if (indices_size != 1) {
    printf("[verify tx proof] more than one leaf, len %d", indices_size);
    return GW_FATAL_INVALID_DATA;
  }
  mol_seg_res_t tx_leaf_index_res = MolReader_Uint32Vec_get(&indices_seg, 0);
  if (tx_leaf_index_res.errno != MOL_OK) {
    printf("[verify tx proof] invalid tx leaf index");
    return GW_FATAL_INVALID_DATA;
  }

  uint32_t leaf_index = 0;
  _gw_fast_memcpy((uint8_t *)(&leaf_index), tx_leaf_index_res.seg.ptr,
                  sizeof(uint32_t));
  printf("[verify tx proof] leaf index %d", leaf_index);

  /* calculate leaf hash */
  uint8_t leaf_hash[32] = {0};

  blake2b_state blake2b_ctx;
  blake2b_init(&blake2b_ctx, 32);
  blake2b_update(&blake2b_ctx, l2tx_seg->ptr, l2tx_seg->size);
  blake2b_final(&blake2b_ctx, leaf_hash, 32);

  blake2b_init(&blake2b_ctx, 32);
  blake2b_update(&blake2b_ctx, (uint8_t *)&tx_index, 4);
  blake2b_update(&blake2b_ctx, leaf_hash, 32);
  blake2b_final(&blake2b_ctx, leaf_hash, 32);

  mol_seg_t lemmas_seg = MolReader_CKBMerkleProof_get_lemmas(proof_seg);
  uint32_t lemmas_size = MolReader_Byte32Vec_length(&lemmas_seg);
  printf("[verify tx proof] lemmas size %d", lemmas_size);

  uint8_t *left;
  uint8_t *right;
  mol_seg_res_t lemma_res;

  for (uint32_t i = 0; i < lemmas_size; i++) {
    lemma_res = MolReader_Byte32Vec_get(&lemmas_seg, i);
    if (lemma_res.errno != MOL_OK) {
      printf("[verify tx proof] invalid proof lemma idx %d", i);
      return GW_FATAL_INVALID_DATA;
    }
    if (lemma_res.seg.size != 32) {
      printf("[verify tx proof] invalid proof lemma size, idx %d", i);
      return GW_FATAL_INVALID_DATA;
    }

    if (_gw_cbmt_is_left(leaf_index)) {
      left = leaf_hash;
      right = lemma_res.seg.ptr;
    } else {
      left = lemma_res.seg.ptr;
      right = leaf_hash;
    }

    blake2b_init(&blake2b_ctx, 32);
    blake2b_update(&blake2b_ctx, left, 32);
    blake2b_update(&blake2b_ctx, right, 32);
    blake2b_final(&blake2b_ctx, leaf_hash, 32);

    /* move to parent */
    leaf_index = (leaf_index - 1) / 2;
    printf("[verify tx proof] leaf parent index %d", leaf_index);
  }

  return memcmp(root_seg->ptr, leaf_hash, 32);
}

/*
 * Load transaction checkpoints
 */
int _load_tx_checkpoint(mol_seg_t *raw_l2block_seg, uint32_t tx_index,
                        uint8_t prev_tx_checkpoint[32],
                        uint8_t post_tx_checkpoint[32]) {
  mol_seg_t submit_withdrawals_seg =
      MolReader_RawL2Block_get_submit_withdrawals(raw_l2block_seg);
  mol_seg_t withdrawals_count_seg =
      MolReader_SubmitWithdrawals_get_withdrawal_count(&submit_withdrawals_seg);
  uint32_t withdrawals_count = 0;
  _gw_fast_memcpy((uint8_t *)(&withdrawals_count), withdrawals_count_seg.ptr,
                  sizeof(uint32_t));

  mol_seg_t checkpoint_list_seg =
      MolReader_RawL2Block_get_state_checkpoint_list(raw_l2block_seg);

  // load prev tx checkpoint
  if (0 == tx_index) {
    mol_seg_t submit_txs_seg =
        MolReader_RawL2Block_get_submit_transactions(raw_l2block_seg);
    mol_seg_t prev_state_checkpoint_seg =
        MolReader_SubmitTransactions_get_prev_state_checkpoint(&submit_txs_seg);
    if (32 != prev_state_checkpoint_seg.size) {
      printf("invalid prev state checkpoint");
      return GW_FATAL_INVALID_DATA;
    }
    _gw_fast_memcpy(prev_tx_checkpoint, prev_state_checkpoint_seg.ptr, 32);
  } else {
    uint32_t prev_tx_checkpoint_index = withdrawals_count + tx_index - 1;

    mol_seg_res_t checkpoint_res =
        MolReader_Byte32Vec_get(&checkpoint_list_seg, prev_tx_checkpoint_index);
    if (MOL_OK != checkpoint_res.errno || 32 != checkpoint_res.seg.size) {
      printf("invalid prev tx checkpoint");
      return GW_FATAL_INVALID_DATA;
    }
    _gw_fast_memcpy(prev_tx_checkpoint, checkpoint_res.seg.ptr, 32);
  }

  // load post tx checkpoint
  uint32_t post_tx_checkpoint_index = withdrawals_count + tx_index;

  mol_seg_res_t checkpoint_res =
      MolReader_Byte32Vec_get(&checkpoint_list_seg, post_tx_checkpoint_index);
  if (MOL_OK != checkpoint_res.errno || 32 != checkpoint_res.seg.size) {
    printf("invalid post tx checkpoint");
    return GW_FATAL_INVALID_DATA;
  }
  _gw_fast_memcpy(post_tx_checkpoint, checkpoint_res.seg.ptr, 32);
  return 0;
}

/* Load verify transaction witness
 */
int _load_verify_transaction_witness(uint8_t rollup_script_hash[32],
                                     uint64_t challenge_cell_index,
                                     uint8_t challenged_block_hash[32],
                                     uint32_t tx_index,
                                     uint8_t block_merkle_root[32],
                                     gw_context_t *ctx) {
  /* load witness from challenge cell */
  int ret;
  uint8_t buf[GW_MAX_WITNESS_SIZE];
  uint64_t buf_len = GW_MAX_WITNESS_SIZE;
  ret = ckb_checked_load_witness(buf, &buf_len, 0, challenge_cell_index,
                                 CKB_SOURCE_INPUT);
  if (ret != CKB_SUCCESS) {
    printf("load_verify_transaction_witness: load witness failed");
    return GW_FATAL_INVALID_CONTEXT;
  }
  mol_seg_t witness_seg;
  witness_seg.ptr = buf;
  witness_seg.size = buf_len;
  if (MolReader_WitnessArgs_verify(&witness_seg, false) != MOL_OK) {
    printf("witness is not WitnessArgs format");
    return GW_FATAL_INVALID_DATA;
  }

  /* read VerifyTransactionWitness from witness_args.lock */
  mol_seg_t content_seg = MolReader_WitnessArgs_get_lock(&witness_seg);
  if (MolReader_BytesOpt_is_none(&content_seg)) {
    printf("WitnessArgs has no input field");
    return GW_FATAL_INVALID_DATA;
  }
  mol_seg_t cc_tx_witness_seg = MolReader_Bytes_raw_bytes(&content_seg);
  if (MolReader_CCTransactionWitness_verify(&cc_tx_witness_seg, false) !=
      MOL_OK) {
    printf("input field is not VerifyTransactionWitness");
    return GW_FATAL_INVALID_DATA;
  }

  mol_seg_t raw_l2block_seg =
      MolReader_CCTransactionWitness_get_raw_l2block(&cc_tx_witness_seg);

  /* verify challenged block */
  uint8_t block_hash[32] = {0};

  blake2b_state blake2b_ctx;
  blake2b_init(&blake2b_ctx, 32);
  blake2b_update(&blake2b_ctx, raw_l2block_seg.ptr, raw_l2block_seg.size);
  blake2b_final(&blake2b_ctx, block_hash, 32);
  if (memcmp(block_hash, challenged_block_hash, 32) != 0) {
    printf("block hash mismatched with challenged block hash");
    return GW_FATAL_INVALID_DATA;
  }

  /* verify tx is challenge target */
  mol_seg_t l2tx_seg =
      MolReader_CCTransactionWitness_get_l2tx(&cc_tx_witness_seg);
  mol_seg_t raw_l2tx_seg = MolReader_L2Transaction_get_raw(&l2tx_seg);

  /* verify tx merkle proof */
  mol_seg_t submit_txs_seg =
      MolReader_RawL2Block_get_submit_transactions(&raw_l2block_seg);
  mol_seg_t tx_witness_root_seg =
      MolReader_SubmitTransactions_get_tx_witness_root(&submit_txs_seg);
  mol_seg_t tx_proof_seg =
      MolReader_CCTransactionWitness_get_tx_proof(&cc_tx_witness_seg);

  ret = _gw_verify_cbmt_tx_proof(&tx_proof_seg, &tx_witness_root_seg, tx_index,
                                 &l2tx_seg);
  if (ret != 0) {
    printf("failed to verify tx witness root ret %d", ret);
    return GW_FATAL_SMT_VERIFY;
  }

  /* load transaction context */
  ret = gw_parse_transaction_context(&ctx->transaction_context, &raw_l2tx_seg);
  if (ret != 0) {
    printf("parse l2 transaction failed");
    return ret;
  }

  /* load block info */
  mol_seg_t number_seg = MolReader_RawL2Block_get_number(&raw_l2block_seg);
  uint64_t challenged_block_number = 0;
  _gw_fast_memcpy((uint8_t *)(&challenged_block_number), number_seg.ptr,
                  sizeof(uint64_t));
  mol_seg_t timestamp_seg =
      MolReader_RawL2Block_get_timestamp(&raw_l2block_seg);
  mol_seg_t block_producer_seg =
      MolReader_RawL2Block_get_block_producer(&raw_l2block_seg);
  mol_seg_t raw_block_producer_seg =
      MolReader_Bytes_raw_bytes(&block_producer_seg);
  _gw_fast_memcpy((uint8_t *)(&ctx->block_info.number), number_seg.ptr,
                  sizeof(uint64_t));
  _gw_fast_memcpy((uint8_t *)(&ctx->block_info.timestamp), timestamp_seg.ptr,
                  sizeof(uint64_t));
  /* parse block producer */
  ret = _gw_parse_addr(raw_block_producer_seg.ptr, raw_block_producer_seg.size,
                       &ctx->block_info.block_producer);
  if (ret != 0) {
    return ret;
  }

  /* load block hashes */
  mol_seg_t block_hashes_seg =
      MolReader_CCTransactionWitness_get_block_hashes(&cc_tx_witness_seg);
  uint32_t block_hashes_size =
      MolReader_BlockHashEntryVec_length(&block_hashes_seg);
  smt_state_init(&ctx->block_hashes_state, ctx->block_hashes_pairs,
                 GW_MAX_GET_BLOCK_HASH_DEPTH);
  uint64_t max_block_number = 0;
  if (challenged_block_number > 1) {
    max_block_number = challenged_block_number - 1;
  }
  uint64_t min_block_number = 0;
  if (challenged_block_number > GW_MAX_GET_BLOCK_HASH_DEPTH) {
    min_block_number = challenged_block_number - GW_MAX_GET_BLOCK_HASH_DEPTH;
  }

  for (uint32_t i = 0; i < block_hashes_size; i++) {
    mol_seg_res_t block_hash_entry_res =
        MolReader_BlockHashEntryVec_get(&block_hashes_seg, i);
    if (block_hash_entry_res.errno != MOL_OK) {
      printf("invalid block hash entry");
      return GW_FATAL_INVALID_DATA;
    }
    mol_seg_t num_seg =
        MolReader_BlockHashEntry_get_number(&block_hash_entry_res.seg);
    uint64_t block_number = 0;
    _gw_fast_memcpy((uint8_t *)(&block_number), num_seg.ptr, sizeof(uint64_t));
    if (block_number < min_block_number || block_number > max_block_number) {
      printf("invalid number in block hashes");
      return GW_FATAL_INVALID_DATA;
    }
    mol_seg_t hash_seg =
        MolReader_BlockHashEntry_get_hash(&block_hash_entry_res.seg);
    uint8_t key[32] = {0};
    _gw_block_smt_key(key, block_number);
    ret = smt_state_insert(&ctx->block_hashes_state, key, hash_seg.ptr);
    if (ret != 0) {
      printf("failed to insert into smt, ret: %d", ret);
      return GW_FATAL_SMT_STORE;
    }
  }
  /* Merkle proof */
  if (block_hashes_size > 0) {
    mol_seg_t block_hashes_proof_seg =
        MolReader_CCTransactionWitness_get_block_hashes_proof(
            &cc_tx_witness_seg);
    smt_state_normalize(&ctx->block_hashes_state);
    ret = smt_verify(block_merkle_root, &ctx->block_hashes_state,
                     block_hashes_proof_seg.ptr, block_hashes_proof_seg.size);
    if (ret != 0) {
      printf("failed to verify block merkle root and block hashes");
      return GW_FATAL_SMT_VERIFY;
    }
  }

  /* load kv state */
  mol_seg_t kv_state_seg =
      MolReader_CCTransactionWitness_get_kv_state(&cc_tx_witness_seg);
  uint32_t kv_pairs_len = MolReader_KVPairVec_length(&kv_state_seg);
  if (kv_pairs_len > GW_MAX_KV_PAIRS) {
    printf("too many key/value pair");
    return GW_FATAL_INVALID_DATA;
  }
  /* initialize kv state */
  smt_state_init(&ctx->kv_state, ctx->kv_pairs, GW_MAX_KV_PAIRS);
  for (uint32_t i = 0; i < kv_pairs_len; i++) {
    mol_seg_res_t kv_res = MolReader_KVPairVec_get(&kv_state_seg, i);
    if (kv_res.errno != MOL_OK) {
      printf("invalid kv pairs");
      return GW_FATAL_INVALID_DATA;
    }
    mol_seg_t key_seg = MolReader_KVPair_get_k(&kv_res.seg);
    mol_seg_t value_seg = MolReader_KVPair_get_v(&kv_res.seg);
    ret = smt_state_insert(&ctx->kv_state, key_seg.ptr, value_seg.ptr);
    if (ret != 0) {
      printf("failed to insert smt kv pair, ret: %d", ret);
      return GW_FATAL_SMT_STORE;
    }
  }

  /* load kv state proof */
  mol_seg_t kv_state_proof_seg =
      MolReader_CCTransactionWitness_get_kv_state_proof(&cc_tx_witness_seg);
  mol_seg_t kv_state_proof_bytes_seg =
      MolReader_Bytes_raw_bytes(&kv_state_proof_seg);
  if (kv_state_proof_bytes_seg.size > GW_MAX_KV_PROOF_SIZE) {
    printf("kv state proof is too long");
    return GW_FATAL_BUFFER_OVERFLOW;
  }
  _gw_fast_memcpy(ctx->kv_state_proof, kv_state_proof_bytes_seg.ptr,
                  kv_state_proof_bytes_seg.size);
  ctx->kv_state_proof_size = kv_state_proof_bytes_seg.size;

  /* load tx checkpoint */
  ret = _load_tx_checkpoint(&raw_l2block_seg, tx_index, ctx->prev_tx_checkpoint,
                            ctx->post_tx_checkpoint);
  if (ret != 0) {
    return ret;
  }

  mol_seg_t account_count_seg =
      MolReader_CCTransactionWitness_get_account_count(&cc_tx_witness_seg);
  _gw_fast_memcpy((uint8_t *)(&ctx->account_count), account_count_seg.ptr,
                  sizeof(uint32_t));

  /* load prev account state */
  mol_seg_t prev_account_seg =
      MolReader_RawL2Block_get_prev_account(&raw_l2block_seg);
  mol_seg_t prev_merkle_root_seg =
      MolReader_AccountMerkleState_get_merkle_root(&prev_account_seg);
  mol_seg_t prev_count_seg =
      MolReader_AccountMerkleState_get_count(&prev_account_seg);
  _gw_fast_memcpy(ctx->prev_account.merkle_root, prev_merkle_root_seg.ptr, 32);
  _gw_fast_memcpy((uint8_t *)(&ctx->prev_account.count), prev_count_seg.ptr,
                  sizeof(uint32_t));
  /* load post account state */
  mol_seg_t post_account_seg =
      MolReader_RawL2Block_get_post_account(&raw_l2block_seg);
  mol_seg_t post_merkle_root_seg =
      MolReader_AccountMerkleState_get_merkle_root(&post_account_seg);
  mol_seg_t post_count_seg =
      MolReader_AccountMerkleState_get_count(&post_account_seg);
  _gw_fast_memcpy(ctx->post_account.merkle_root, post_merkle_root_seg.ptr, 32);
  _gw_fast_memcpy((uint8_t *)(&ctx->post_account.count), post_count_seg.ptr,
                  sizeof(uint32_t));

  /* load scripts */
  mol_seg_t scripts_seg =
      MolReader_CCTransactionWitness_get_scripts(&cc_tx_witness_seg);
  uint32_t entries_size = MolReader_ScriptVec_length(&scripts_seg);
  if (entries_size > GW_MAX_SCRIPT_ENTRIES_SIZE) {
    printf("script size is exceeded maximum");
    return GW_FATAL_BUFFER_OVERFLOW;
  }
  ctx->script_entries_size = 0;
  for (uint32_t i = 0; i < entries_size; i++) {
    gw_script_entry_t entry = {0};
    mol_seg_res_t script_res = MolReader_ScriptVec_get(&scripts_seg, i);
    if (script_res.errno != MOL_OK) {
      printf("invalid script entry format");
      return GW_FATAL_INVALID_DATA;
    }
    if (script_res.seg.size > GW_MAX_SCRIPT_SIZE) {
      printf("invalid script entry format");
      return GW_FATAL_INVALID_DATA;
    }

    /* copy script to entry */
    _gw_fast_memcpy(entry.script, script_res.seg.ptr, script_res.seg.size);
    entry.script_len = script_res.seg.size;

    /* copy script hash to entry */
    blake2b_state blake2b_ctx;
    blake2b_init(&blake2b_ctx, 32);
    blake2b_update(&blake2b_ctx, script_res.seg.ptr, script_res.seg.size);
    blake2b_final(&blake2b_ctx, entry.hash, 32);

    /* insert entry */
    _gw_fast_memcpy(&ctx->scripts[ctx->script_entries_size], &entry,
                    sizeof(gw_script_entry_t));
    ctx->script_entries_size += 1;
  }

  /* load data */
  mol_seg_t load_data_seg =
      MolReader_CCTransactionWitness_get_load_data(&cc_tx_witness_seg);
  entries_size = MolReader_BytesVec_length(&load_data_seg);
  if (entries_size > GW_MAX_LOAD_DATA_ENTRIES_SIZE) {
    printf("load data size is exceeded maximum");
    return GW_FATAL_BUFFER_OVERFLOW;
  }
  ctx->load_data_entries_size = 0;
  for (uint32_t i = 0; i < entries_size; i++) {
    gw_load_data_entry_t entry = {0};
    mol_seg_res_t load_data_res = MolReader_BytesVec_get(&load_data_seg, i);
    if (load_data_res.errno != MOL_OK) {
      printf("invalid load data entry format");
      return GW_FATAL_INVALID_DATA;
    }

    mol_seg_t raw_data_seg = MolReader_Bytes_raw_bytes(&load_data_res.seg);
    if (raw_data_seg.size > GW_MAX_DATA_SIZE) {
      printf("load data too long");
      return GW_FATAL_INVALID_DATA;
    }

    /* copy load data to entry */
    entry.data = (uint8_t *)malloc(raw_data_seg.size);
    if (NULL == entry.data) {
      printf("malloc load data failed");
      return GW_FATAL_BUFFER_OVERFLOW;
    }
    _gw_fast_memcpy(entry.data, raw_data_seg.ptr, raw_data_seg.size);
    entry.data_len = raw_data_seg.size;

    /* copy script hash to entry */
    blake2b_state blake2b_ctx;
    blake2b_init(&blake2b_ctx, 32);
    blake2b_update(&blake2b_ctx, raw_data_seg.ptr, raw_data_seg.size);
    blake2b_final(&blake2b_ctx, entry.hash, 32);

    /* insert entry */
    _gw_fast_memcpy(&ctx->load_data[ctx->load_data_entries_size], &entry,
                    sizeof(gw_load_data_entry_t));
    ctx->load_data_entries_size += 1;
  }

  /* load return data hash */
  mol_seg_t return_data_hash_seg =
      MolReader_CCTransactionWitness_get_return_data_hash(&cc_tx_witness_seg);
  _gw_fast_memcpy(ctx->return_data_hash, return_data_hash_seg.ptr, 32);

  return 0;
}

/* check that an account script is allowed */
int _gw_check_account_script_is_allowed(uint8_t rollup_script_hash[32],
                                        mol_seg_t *script_seg,
                                        mol_seg_t *rollup_config_seg) {
  if (MolReader_Script_verify(script_seg, false) != MOL_OK) {
    printf("[check account script] script invalid format");
    return GW_ERROR_INVALID_ACCOUNT_SCRIPT;
  }

  if (script_seg->size > GW_MAX_SCRIPT_SIZE) {
    printf("[check account script] script size is too large");
    return GW_ERROR_INVALID_ACCOUNT_SCRIPT;
  }

  /* check hash type */
  mol_seg_t hash_type_seg = MolReader_Script_get_hash_type(script_seg);
  if (*(uint8_t *)hash_type_seg.ptr != SCRIPT_HASH_TYPE_TYPE) {
    printf("[check account script]  hash type is not 'type'");
    return GW_ERROR_UNKNOWN_SCRIPT_CODE_HASH;
  }

  /* check script.args */
  mol_seg_t args_seg = MolReader_Script_get_args(script_seg);
  mol_seg_t raw_args_seg = MolReader_Bytes_raw_bytes(&args_seg);
  if (raw_args_seg.size < 32) {
    printf("[check account script] script args is less than 32 bytes");
    return GW_ERROR_INVALID_ACCOUNT_SCRIPT;
  }
  if (memcmp(rollup_script_hash, raw_args_seg.ptr, 32) != 0) {
    printf("[check account script] args is not start with rollup_script_hash");
    return GW_ERROR_INVALID_ACCOUNT_SCRIPT;
  }

  /* check code_hash */
  mol_seg_t script_code_hash_seg = MolReader_Script_get_code_hash(script_seg);
  if (script_code_hash_seg.size != 32) {
    return GW_FATAL_INVALID_DATA;
  }

  /* check allowed EOA list */
  mol_seg_t allowed_eoa_list_seg =
      MolReader_RollupConfig_get_allowed_eoa_type_hashes(rollup_config_seg);
  uint32_t len = MolReader_AllowedTypeHashVec_length(&allowed_eoa_list_seg);
  for (uint32_t i = 0; i < len; i++) {
    mol_seg_res_t allowed_type_hash_res =
        MolReader_AllowedTypeHashVec_get(&allowed_eoa_list_seg, i);

    if (allowed_type_hash_res.errno != MOL_OK) {
      printf("[check account script] failed to get EOA code hash");
      return GW_FATAL_INVALID_DATA;
    }

    mol_seg_t code_hash_seg =
        MolReader_AllowedTypeHash_get_hash(&allowed_type_hash_res.seg);
    if (code_hash_seg.size != script_code_hash_seg.size) {
      printf(
          "[check account script] failed to get EOA code hash, size mismatch");
      return GW_FATAL_INVALID_DATA;
    }

    if (memcmp(code_hash_seg.ptr, script_code_hash_seg.ptr,
               script_code_hash_seg.size) == 0) {
      /* found a valid code_hash */
      printf("[check account script] script is EOA");
      return 0;
    }
  }

  /* check allowed contract list */
  mol_seg_t allowed_contract_list_seg =
      MolReader_RollupConfig_get_allowed_contract_type_hashes(
          rollup_config_seg);
  len = MolReader_AllowedTypeHashVec_length(&allowed_contract_list_seg);
  for (uint32_t i = 0; i < len; i++) {
    mol_seg_res_t allowed_type_hash_res =
        MolReader_AllowedTypeHashVec_get(&allowed_contract_list_seg, i);
    if (allowed_type_hash_res.errno != MOL_OK) {
      printf("[check account script] failed to get contract code hash");
      return GW_FATAL_INVALID_DATA;
    }

    mol_seg_t code_hash_seg =
        MolReader_AllowedTypeHash_get_hash(&allowed_type_hash_res.seg);
    if (code_hash_seg.size != script_code_hash_seg.size) {
      printf(
          "[check account script] failed to get contract code hash, size "
          "mismatch");
      return GW_FATAL_INVALID_DATA;
    }

    if (memcmp(code_hash_seg.ptr, script_code_hash_seg.ptr,
               script_code_hash_seg.size) == 0) {
      /* found a valid code_hash */
      printf("[check account script] script is contract");
      return 0;
    }
  }

  /* script is not allowed */
  printf("[check account script] unknown code_hash");
  return GW_ERROR_UNKNOWN_SCRIPT_CODE_HASH;
}

/* block smt key */
void _gw_block_smt_key(uint8_t key[32], uint64_t number) {
  _gw_fast_memcpy(key, (uint8_t *)&number, 8);
}

/*
 * To prevent others consume the cell,
 * an owner_lock_hash(32 bytes) is put in the current cell's data,
 * this function checks that at least an input cell's lock_hash equals to the
 * owner_lock_hash, thus, we can make sure current cell is unlocked by the
 * owner, otherwise this function return an error.
 */
int _check_owner_lock_hash() {
  /* read data from current cell */
  uint8_t owner_lock_hash[32] = {0};
  uint64_t len = 32;
  int ret =
      ckb_load_cell_data(owner_lock_hash, &len, 0, 0, CKB_SOURCE_GROUP_INPUT);
  if (ret != 0) {
    printf("check owner lock hash failed, can't load cell data, ret: %d", ret);
    return GW_FATAL_INVALID_CONTEXT;
  }
  if (len != 32) {
    printf("check owner lock hash failed, invalid data len: %ld", len);
    return GW_FATAL_INVALID_DATA;
  }
  /* look for owner cell */
  size_t current = 0;
  while (true) {
    len = 32;
    uint8_t lock_hash[32] = {0};

    ret = ckb_load_cell_by_field(lock_hash, &len, 0, current, CKB_SOURCE_INPUT,
                                 CKB_CELL_FIELD_LOCK_HASH);

    if (ret != 0) {
      printf(
          "check owner lock hash failed: failed to load cell lock_hash ret: "
          "%d",
          ret);
      return GW_FATAL_INVALID_CONTEXT;
    }
    if (memcmp(lock_hash, owner_lock_hash, 32) == 0) {
      /* found owner lock cell */
      return 0;
    }
    current++;
  }
  printf("failed to check owner lock");
  return GW_ERROR_NOT_FOUND;
}

int _gw_calculate_state_checkpoint(uint8_t buffer[32], const smt_state_t *state,
                                   const uint8_t *proof, uint32_t proof_length,
                                   uint32_t account_count) {
  uint8_t root[32];
  int ret = smt_calculate_root(root, state, proof, proof_length);
  if (0 != ret) {
    printf(
        "_gw_calculate_state_check_point: failed to calculate kv state "
        "root ret: %d",
        ret);
    return GW_FATAL_SMT_CALCULATE_ROOT;
  }

  blake2b_state blake2b_ctx;
  blake2b_init(&blake2b_ctx, 32);
  blake2b_update(&blake2b_ctx, root, 32);
  blake2b_update(&blake2b_ctx, &account_count, sizeof(uint32_t));
  blake2b_final(&blake2b_ctx, buffer, 32);

  return 0;
}

int _gw_verify_checkpoint(const uint8_t checkpoint[32],
                          const smt_state_t *state, const uint8_t *proof,
                          uint32_t proof_length, uint32_t account_count) {
  uint8_t proof_checkpoint[32];
  int ret = _gw_calculate_state_checkpoint(proof_checkpoint, state, proof,
                                           proof_length, account_count);
  if (0 != ret) {
    return ret;
  }
  if (0 != memcmp(proof_checkpoint, checkpoint, 32)) {
    return GW_FATAL_INVALID_CHECK_POINT;
  }
  return 0;
}

int gw_context_init(gw_context_t *ctx) {
  /* check owner lock */
  int ret = _check_owner_lock_hash();
  if (ret != 0) {
    printf("gw_context_init: not found owner lock, ret: %d", ret);
    return ret;
  }

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
  ctx->sys_log = sys_log;
  ctx->sys_pay_fee = sys_pay_fee;
  ctx->sys_get_registry_address_by_script_hash =
      _gw_get_registry_address_by_script_hash;
  ctx->sys_get_script_hash_by_registry_address =
      _gw_get_script_hash_by_registry_address;
  ctx->sys_snapshot = sys_snapshot;
  ctx->sys_revert = sys_revert;
  ctx->sys_check_sudt_addr_permission = sys_check_sudt_addr_permission;
  ctx->_internal_load_raw = _internal_load_raw;
  ctx->_internal_store_raw = _internal_store_raw;

  /* initialize context */
  uint8_t rollup_script_hash[32] = {0};
  ret = _load_rollup_script_hash(rollup_script_hash);
  if (ret != 0) {
    printf("failed to load rollup script hash");
    return ret;
  }
  /* set ctx->rollup_script_hash */
  _gw_fast_memcpy(ctx->rollup_script_hash, rollup_script_hash, 32);
  uint64_t rollup_cell_index = 0;
  ret = _find_cell_by_type_hash(rollup_script_hash, CKB_SOURCE_INPUT,
                                &rollup_cell_index);
  if (ret == GW_ERROR_NOT_FOUND) {
    /* exit execution with 0 if we are not in a challenge */
    printf(
        "gw_context_init: can't found rollup cell from inputs which "
        "means we are not in a "
        "challenge, unlock cell without execution script");
    ckb_exit(0);
  } else if (ret != 0) {
    printf("gw_context_init: failed to load rollup cell index, ret: %d", ret);
    return GW_FATAL_INVALID_CONTEXT;
  }
  uint64_t challenge_cell_index = 0;
  uint8_t challenged_block_hash[32] = {0};
  uint8_t block_merkle_root[32] = {0};
  ret = _load_verification_context(
      rollup_script_hash, rollup_cell_index, CKB_SOURCE_INPUT,
      &challenge_cell_index, challenged_block_hash, block_merkle_root,
      &ctx->tx_index, ctx->rollup_config, &ctx->rollup_config_size);
  if (ret != 0) {
    printf("failed to load verification context");
    return ret;
  }

  /* load context fields */
  ret = _load_verify_transaction_witness(
      rollup_script_hash, challenge_cell_index, challenged_block_hash,
      ctx->tx_index, block_merkle_root, ctx);
  if (ret != 0) {
    printf("failed to load verify transaction witness");
    return ret;
  }

  /* verify kv_state merkle proof */
  smt_state_normalize(&ctx->kv_state);
  ret = _gw_verify_checkpoint(ctx->prev_tx_checkpoint, &ctx->kv_state,
                              ctx->kv_state_proof, ctx->kv_state_proof_size,
                              ctx->account_count);
  if (ret != 0) {
    printf("failed to merkle verify prev tx checkpoint");
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

  uint8_t return_data_hash[32] = {0};
  blake2b_state blake2b_ctx;
  blake2b_init(&blake2b_ctx, 32);
  blake2b_update(&blake2b_ctx, ctx->receipt.return_data,
                 ctx->receipt.return_data_len);
  blake2b_final(&blake2b_ctx, return_data_hash, 32);
  if (memcmp(return_data_hash, ctx->return_data_hash, 32) != 0) {
    printf("return data hash not match");
    return GW_FATAL_MISMATCH_RETURN_DATA;
  }

  smt_state_normalize(&ctx->kv_state);
  ret = _gw_verify_checkpoint(ctx->post_tx_checkpoint, &ctx->kv_state,
                              ctx->kv_state_proof, ctx->kv_state_proof_size,
                              ctx->account_count);
  if (ret != 0) {
    printf("failed to merkle verify post tx checkpoint");
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
