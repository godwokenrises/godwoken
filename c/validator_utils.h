#ifndef GW_VALIDATOR_H_
#define GW_VALIDATOR_H_

#include "ckb_syscalls.h"
#include "common.h"

#define MAX_BUF_SIZE 65536
/* 2048 * (32 + 32 + 8) = 147456 Byte (~144KB)*/
#define KV_STATE_CAPACITY 2048

typedef struct  {
  uint8_t merkle_root[32];
  uint32_t count;
} gw_account_merkle_state_t;

typedef struct {
  uint8_t block_hash[32];
  uint64_t block_number;
  uint32_t tx_index;
} gw_start_challenge_t;

/* NOTE: all field except must be pointer or const value */
typedef struct gw_context_t {
  /* verification context */
  gw_transaction_context_t transaction_context;
  gw_block_info_t block_info;
  /* layer2 syscalls */
  gw_load_fn sys_load;
  gw_load_nonce_fn sys_load_nonce;
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

  /* validator specific context */
  gw_account_merkle_state_t *prev_account;
  gw_account_merkle_state_t *post_account;
  gw_start_challenge_t *start_challenge;
  gw_state_t *kv_state;
  /* SMT proof */
  uint8_t *kv_state_proof;
  size_t kv_state_proof_size;
  /* To proof the block is in the chain */
  uint8_t *block_merkle_root;
  uint8_t *block_proof;
  size_t block_proof_size;


  /* merkle root of L2Transaction list */
  uint8_t *tx_witness_root;
  /* hash of L2Transaction */
  uint8_t *tx_hash;
  /* transaction proof */
  uint8_t *tx_proof;
  size_t tx_proof_size;

  /* The script of entrance account */
  uint8_t *entrance_account_script;
  size_t entrance_account_script_size;
  uint32_t entrance_account_id;
} gw_context_t;

int sys_load(gw_context_t *ctx, uint32_t account_id, const uint8_t key[GW_KEY_BYTES],
             uint8_t value[GW_VALUE_BYTES]) {
  if (ctx == NULL) {
    return GW_ERROR_INVALID_CONTEXT;
  }
  uint8_t raw_key[GW_KEY_BYTES] = {0};
  gw_build_account_key(account_id, key, raw_key);
  return gw_state_fetch(ctx->kv_state, raw_key, value);
}
int sys_store(gw_context_t *ctx, uint32_t account_id, const uint8_t key[GW_KEY_BYTES],
              const uint8_t value[GW_VALUE_BYTES]) {
  if (ctx == NULL) {
    return GW_ERROR_INVALID_CONTEXT;
  }
  uint8_t raw_key[GW_KEY_BYTES];
  gw_build_account_key(account_id, key, raw_key);
  return gw_state_insert(ctx->kv_state, raw_key, value);
}

int sys_load_nonce(gw_context_t *ctx, uint32_t account_id, uint8_t value[GW_VALUE_BYTES]) {
  if (ctx == NULL) {
    return GW_ERROR_INVALID_CONTEXT;
  }
  uint8_t key[32];
  gw_build_nonce_key(account_id, key);
  return gw_state_fetch(ctx->kv_state, key, value);
}

/* set call return data */
int sys_set_program_return_data(gw_context_t *ctx, uint8_t *data, uint32_t len) {
  /* TODO: Do nothing? */
  return 0;
}

/* Get account id by account script_hash */
int sys_get_account_id_by_script_hash(gw_context_t *ctx, uint8_t script_hash[32],
                                      uint32_t *account_id) {
  if (ctx == NULL) {
    return GW_ERROR_INVALID_CONTEXT;
  }
  uint8_t raw_key[32];
  uint8_t value[32];
  gw_build_script_hash_to_account_id_key(script_hash, raw_key);
  int ret = gw_state_fetch(ctx->kv_state, raw_key, value);
  if (ret != 0) {
    return ret;
  }
  for (int i = 4; i < 32; i++) {
    if (value[i] != 0) {
      ckb_debug("Invalid account id value");
      return -1;
    }
  }
  *account_id = *((uint32_t *)value);
  return 0;
}

/* Get account script_hash by account id */
int sys_get_script_hash_by_account_id(gw_context_t *ctx, uint32_t account_id,
                                      uint8_t script_hash[32]) {
  if (ctx == NULL) {
    return GW_ERROR_INVALID_CONTEXT;
  }
  uint8_t raw_key[32];
  gw_build_account_field_key(account_id, GW_ACCOUNT_SCRIPT_HASH, raw_key);
  return gw_state_fetch(ctx->kv_state, raw_key, script_hash);
}

/* Get account script by account id */
int sys_get_account_script(gw_context_t *ctx, uint32_t account_id, uint32_t *len,
                         uint32_t offset, uint8_t *script) {
  if (ctx == NULL) {
    return GW_ERROR_INVALID_CONTEXT;
  }
  int ret;

  if (ctx->entrance_account_id == account_id) {
    /* verify the script hash */
    uint8_t script_hash[32];
    ret = sys_get_script_hash_by_account_id(ctx, account_id, script_hash);
    if (ret != 0) {
      return ret;
    }
    uint8_t calculated_script_hash[32];
    blake2b_state blake2b_ctx;
    blake2b_init(&blake2b_ctx, 32);
    blake2b_update(&blake2b_ctx,
                   ctx->entrance_account_script,
                   ctx->entrance_account_script_size);
    blake2b_final(&blake2b_ctx, calculated_script_hash, 32);

    if (memcmp(script_hash, calculated_script_hash, 32) != 0) {
      ckb_debug("verify entrance account script hash failed");
      return -1;
    }

    /* return account script */
    size_t new_len;
    size_t data_len = ctx->entrance_account_script_size;
    if (offset >= data_len) {
      new_len = 0;
    } else if ((offset + *len) > data_len) {
      new_len = data_len - offset;
    } else {
      new_len = *len;
    }
    if (new_len > 0) {
      memcpy(script, ctx->entrance_account_script + offset, new_len);
    }
    return 0;
  } else {
    ckb_debug("account script not found for given account id");
    return -1;
  }
}
/* Store data by data hash */
int sys_store_data(gw_context_t *ctx,
                 uint32_t data_len,
                 uint8_t *data) {
  /* TODO: any verification ? */
  /* do nothing for now */
  return 0;
}
/* Load data by data hash */
int sys_load_data(gw_context_t *ctx, uint8_t data_hash[32],
                 uint32_t *len, uint32_t offset, uint8_t *data) {
  int ret;
  size_t index = 0;
  uint64_t hash_len = 32;
  uint8_t hash[32];
  while (1) {
    ret = ckb_load_cell_by_field(hash, &hash_len, 0, index, CKB_SOURCE_CELL_DEP, CKB_CELL_FIELD_DATA_HASH);
    if (ret == CKB_ITEM_MISSING) {
      ckb_debug("not found cell data by data hash");
      return -1;
    } else if (ret == CKB_SUCCESS) {
      if (memcmp(hash, data_hash, 32) == 0) {
        uint64_t data_len = (uint64_t)*len;
        ret = ckb_load_cell_data(data, &data_len, offset, index, CKB_SOURCE_CELL_DEP);
        if (ret != CKB_SUCCESS) {
          ckb_debug("load cell data failed");
          return -1;
        }
        *len = (uint32_t)data_len;
        return 0;
      }
    } else {
      ckb_debug("load cell data hash failed");
      return -1;
    }
    index += 1;
  }
  /* dead code */
  return -1;
}

int sys_create(gw_context_t *ctx, uint8_t *script, uint32_t script_len,
               uint32_t *account_id) {
  if (ctx == NULL) {
    return GW_ERROR_INVALID_CONTEXT;
  }
  int ret;
  uint32_t id = ctx->prev_account->count;

  uint8_t nonce_key[32];
  uint8_t nonce_value[32];
  gw_build_account_field_key(id, GW_ACCOUNT_NONCE, nonce_key);
  memset(nonce_value, 0, 32);
  ret = gw_state_insert(ctx->kv_state, nonce_key, nonce_value);
  if (ret != 0) {
    return -1;
  }

  uint8_t script_hash[32];
  uint8_t script_hash_key[32];
  blake2b_state blake2b_ctx;
  blake2b_init(&blake2b_ctx, 32);
  blake2b_update(&blake2b_ctx, script, script_len);
  blake2b_final(&blake2b_ctx, script_hash, 32);
  gw_build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH, script_hash_key);
  ret = gw_state_insert(ctx->kv_state, script_hash_key, script_hash);
  if (ret != 0) {
    return -1;
  }

  uint8_t hash_to_id_key[32];
  uint8_t hash_to_id_value[32];
  gw_build_script_hash_to_account_id_key(script_hash, hash_to_id_key);
  memcpy(hash_to_id_value, (uint8_t *)(&id), 4);
  ret = gw_state_insert(ctx->kv_state, hash_to_id_key, hash_to_id_value);
  if (ret != 0) {
    return -1;
  }

  /* TODO: how to verify new_scripts? */

  ctx->prev_account->count += 1;

  return 0;
}

int sys_log(gw_context_t *ctx, uint32_t account_id, uint32_t data_length,
            const uint8_t *data) {
  /* do nothing */
  return 0;
}

/* FIXME: Load and verify rollup cell */
int load_rollup_cell() {
  return -1;
}
/* Load and verify challenge cell */
int load_challenge_cell(gw_context_t *ctx) {
  if (ctx == NULL) {
    return GW_ERROR_INVALID_CONTEXT;
  }

  int ret;
  uint8_t buf[512];
  uint64_t buf_len = 512;
  ret = ckb_load_cell_data(buf, &buf_len, 0, 0, CKB_SOURCE_GROUP_INPUT);
  if (ret != 0) {
    return ret;
  }
  mol_seg_t cell_seg;
  cell_seg.ptr = buf;
  cell_seg.size = buf_len;
  if (MolReader_StartChallenge_verify(&cell_seg, false) != MOL_OK) {
    ckb_debug("channel cell data is not StartChallenge format");
    return -1;
  }
  mol_seg_t block_hash_seg = MolReader_StartChallenge_get_block_hash(&cell_seg);
  mol_seg_t block_number_seg = MolReader_StartChallenge_get_block_number(&cell_seg);
  mol_seg_t tx_index_seg = MolReader_StartChallenge_get_tx_index(&cell_seg);
  ctx->start_challenge = (gw_start_challenge_t *)malloc(sizeof(gw_start_challenge_t));
  memcpy(ctx->start_challenge->block_hash,
         block_hash_seg.ptr,
         block_hash_seg.size);
  ctx->start_challenge->block_number = *((uint64_t *) block_number_seg.ptr);
  ctx->start_challenge->tx_index = *((uint32_t *) tx_index_seg.ptr);
  return 0;
}

/* Load and verify cancel challenge transaction witness
 *
 * NOTE: current script as challenge cell's lock script
 */
int load_cancel_challenge_witness(gw_context_t *ctx) {
  if (ctx == NULL) {
    return GW_ERROR_INVALID_CONTEXT;
  }

  int ret;
  uint8_t buf[MAX_BUF_SIZE];
  uint64_t buf_len = MAX_BUF_SIZE;
  ret = ckb_load_witness(buf, &buf_len, 0, 0, CKB_SOURCE_GROUP_INPUT);
  if (ret != CKB_SUCCESS) {
    ckb_debug("load witness failed");
    return ret;
  }
  mol_seg_t witness_seg;
  witness_seg.ptr = (uint8_t *)buf;
  witness_seg.size = buf_len;
  if (MolReader_WitnessArgs_verify(&witness_seg, false) != MOL_OK) {
    ckb_debug("witness is not WitnessArgs format");
    return -1;
  }
  mol_seg_t content_seg = MolReader_WitnessArgs_get_input_type(&witness_seg);
  if (MolReader_BytesOpt_is_none(&content_seg)) {
    ckb_debug("WitnessArgs has no input field");
    return -1;
  }
  mol_seg_t cancel_challenge_seg = MolReader_Bytes_raw_bytes(&content_seg);
  if (MolReader_CancelChallenge_verify(&cancel_challenge_seg, false) != MOL_OK) {
    ckb_debug("input field is not CancelChallenge");
    return -1;
  }

  mol_seg_t raw_l2block_seg = MolReader_CancelChallenge_get_raw_l2block(&cancel_challenge_seg);
  mol_seg_t l2tx_seg = MolReader_CancelChallenge_get_l2tx(&cancel_challenge_seg);
  mol_seg_t raw_l2tx_seg = MolReader_L2Transaction_get_raw(&l2tx_seg);

  /* load transaction context */
  gw_transaction_context_t *tx_ctx = &ctx->transaction_context;
  ret = gw_parse_transaction_context(tx_ctx, &raw_l2tx_seg);
  if (ret != 0) {
    ckb_debug("parse l2 transaction failed");
    return ret;
  }
  ctx->tx_hash = (uint8_t *)malloc(32);
  blake2b_state blake2b_ctx;
  blake2b_init(&blake2b_ctx, 32);
  blake2b_update(&blake2b_ctx, l2tx_seg.ptr, l2tx_seg.size);
  blake2b_final(&blake2b_ctx, ctx->tx_hash, 32);
  mol_seg_t submit_transactions_seg = MolReader_RawL2Block_get_submit_transactions(&raw_l2block_seg);
  mol_seg_t tx_witness_root_seg = MolReader_SubmitTransactions_get_tx_witness_root(&submit_transactions_seg);
  ctx->tx_witness_root = (uint8_t *)malloc(32);
  memcpy(ctx->tx_witness_root, tx_witness_root_seg.ptr, 32);

  /* load block info */
  gw_block_info_t *block_info = &ctx->block_info;
  mol_seg_t number_seg = MolReader_RawL2Block_get_number(&raw_l2block_seg);
  mol_seg_t timestamp_seg = MolReader_RawL2Block_get_timestamp(&raw_l2block_seg);
  mol_seg_t aggregator_id_seg = MolReader_RawL2Block_get_aggregator_id(&raw_l2block_seg);
  block_info->number = *((uint32_t *)number_seg.ptr);
  block_info->timestamp = *((uint32_t *)timestamp_seg.ptr);
  block_info->aggregator_id = *((uint32_t *)aggregator_id_seg.ptr);

  /* load tx_proof */
  mol_seg_t tx_proof_seg = MolReader_CancelChallenge_get_tx_proof(&cancel_challenge_seg);
  ctx->tx_proof = (uint8_t *)malloc(tx_proof_seg.size);
  memcpy(ctx->tx_proof,
         tx_proof_seg.ptr,
         tx_proof_seg.size);
  ctx->tx_proof_size = (size_t)tx_proof_seg.size;

  /* load block proof */
  mol_seg_t block_proof_seg = MolReader_CancelChallenge_get_block_proof(&cancel_challenge_seg);
  ctx->block_proof = (uint8_t *)malloc(block_proof_seg.size);
  memcpy(ctx->block_proof,
         block_proof_seg.ptr,
         block_proof_seg.size);
  ctx->block_proof_size = (size_t)block_proof_seg.size;

  /* load kv state */
  mol_seg_t kv_state_seg = MolReader_CancelChallenge_get_kv_state(&cancel_challenge_seg);
  uint32_t kv_length = MolReader_KVPairVec_length(&kv_state_seg);
  if (kv_length > KV_STATE_CAPACITY) {
    ckb_debug("too many key/value pair");
    return -1;
  }
  ctx->kv_state = (gw_state_t *)malloc(sizeof(gw_state_t));
  gw_pair_t *kv_pairs = (gw_pair_t *)malloc(sizeof(gw_pair_t) * KV_STATE_CAPACITY);
  gw_state_init(ctx->kv_state, kv_pairs, KV_STATE_CAPACITY);
  for (uint32_t i = 0; i < kv_length; i ++) {
    mol_seg_res_t seg_res = MolReader_KVPairVec_get(&kv_state_seg, i);
    uint8_t error_num = *(uint8_t *)(&seg_res);
    if (error_num != MOL_OK) {
      return -1;
    }
    mol_seg_t kv_pair_seg = seg_res.seg;
    mol_seg_t key_seg = MolReader_KVPair_get_k(&kv_pair_seg);
    mol_seg_t value_seg = MolReader_KVPair_get_v(&kv_pair_seg);
    gw_state_insert(ctx->kv_state, key_seg.ptr, value_seg.ptr);
  }

  mol_seg_t kv_state_proof_seg = MolReader_CancelChallenge_get_kv_state_proof(&cancel_challenge_seg);
  ctx->kv_state_proof = (uint8_t *)malloc(kv_state_proof_seg.size);
  memcpy(ctx->kv_state_proof,
         kv_state_proof_seg.ptr,
         kv_state_proof_seg.size);
  ctx->kv_state_proof_size = (size_t)kv_state_proof_seg.size;

  /* load entrance account */
  mol_seg_t entrance_account_script_seg = MolReader_CancelChallenge_get_entrance_account_script(&cancel_challenge_seg);
  ctx->entrance_account_script = (uint8_t *)malloc(entrance_account_script_seg.size);
  memcpy(ctx->entrance_account_script,
         entrance_account_script_seg.ptr,
         entrance_account_script_seg.size);
  ctx->entrance_account_script_size = (size_t)entrance_account_script_seg.size;
  ctx->entrance_account_id = tx_ctx->to_id;

  /* load previous account state */
  mol_seg_t prev_account_seg = MolReader_RawL2Block_get_prev_account(&raw_l2block_seg);
  mol_seg_t prev_merkle_root_seg = MolReader_AccountMerkleState_get_merkle_root(&prev_account_seg);
  mol_seg_t prev_count_seg = MolReader_AccountMerkleState_get_count(&prev_account_seg);
  ctx->prev_account = (gw_account_merkle_state_t *)malloc(sizeof(gw_account_merkle_state_t));
  memcpy(ctx->prev_account->merkle_root, prev_merkle_root_seg.ptr, 32);
  ctx->prev_account->count = *((uint32_t *)prev_count_seg.ptr);
  /* load post account state */
  mol_seg_t post_account_seg = MolReader_RawL2Block_get_post_account(&raw_l2block_seg);
  mol_seg_t post_merkle_root_seg = MolReader_AccountMerkleState_get_merkle_root(&post_account_seg);
  mol_seg_t post_count_seg = MolReader_AccountMerkleState_get_count(&post_account_seg);
  ctx->post_account = (gw_account_merkle_state_t *)malloc(sizeof(gw_account_merkle_state_t));
  memcpy(ctx->post_account->merkle_root, post_merkle_root_seg.ptr, 32);
  ctx->post_account->count = *((uint32_t *)post_count_seg.ptr);

  return 0;
}

/* Verify challenged layer 2 transaction is belong to the challenged layer 2 block */
int verify_l2tx(gw_context_t *ctx) {
  if (ctx == NULL) {
    return GW_ERROR_INVALID_CONTEXT;
  }

  int ret;
  gw_state_t kv_state;
  gw_pair_t pair;
  gw_state_init(&kv_state, &pair, 1);
  uint8_t key[32];
  memcpy(key, (uint8_t *)(&ctx->start_challenge->tx_index), 4);
  memset(key + 4, 0, 32 - 4);
  ret = gw_state_insert(&kv_state, key, ctx->tx_hash);
  if (ret != 0) {
    return ret;
  }
  return gw_smt_verify(ctx->tx_witness_root,
                       &kv_state,
                       ctx->tx_proof,
                       ctx->tx_proof_size);
}
/* Verify challenged layer 2 block is belong to the chain */
int verify_l2block(gw_context_t *ctx) {
  /* FIXME: run in which script ? */
  return 0;
}

/* == Verify key value state == */
/* Before execute handle_message verify read values and write old values */
int verify_old_kv_state(gw_context_t *ctx) {
  if (ctx == NULL) {
    return GW_ERROR_INVALID_CONTEXT;
  }
  return gw_smt_verify(ctx->prev_account->merkle_root,
                       ctx->kv_state,
                       ctx->kv_state_proof,
                       ctx->kv_state_proof_size);
}
/* After execute handle_message verify read values and write new values */
int verify_new_kv_state(gw_context_t *ctx) {
  if (ctx == NULL) {
    return GW_ERROR_INVALID_CONTEXT;
  }
  return gw_smt_verify(ctx->post_account->merkle_root,
                       ctx->kv_state,
                       ctx->kv_state_proof,
                       ctx->kv_state_proof_size);
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
  context->sys_store_data = sys_store_data;
  context->sys_load_data = sys_load_data;
  context->sys_log = sys_log;

  /* initialize context */
  int ret;
  ret = load_cancel_challenge_witness(context);
  if (ret != 0) {
    return ret;
  }
  ret = load_challenge_cell(context);
  if (ret != 0) {
    return ret;
  }

  ret = verify_l2block(context);
  if (ret != 0) {
    return ret;
  }
  ret = verify_l2tx(context);
  if (ret != 0) {
    return ret;
  }

  return 0;
}

#endif
