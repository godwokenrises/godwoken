/* Layer2 contract interface */
#define CONTRACT_CONSTRUCTOR_FUNC "gw_construct"
#define CONTRACT_HANDLE_FUNC "gw_handle_message"

/* Common parameters */
#define MAX_PAIRS 1024
#define SCRIPT_SIZE 128
#define WITNESS_SIZE (300 * 1024)
#define CODE_SIZE (512 * 1024)

/* Errors */
#define GW_ERROR_NOT_FOUND 42
#define GW_ERROR_INVALID_DATA 43
#define GW_ERROR_INSUFFICIENT_CAPACITY 44
#define GW_ERROR_MISMATCH_CHANGE_SET 45
#define GW_ERROR_INVALID_CONTEXT 46
#define GW_ERROR_DYNAMIC_LINKING 47

/* Key type */
#define GW_ACCOUNT_KV 0
#define GW_ACCOUNT_NONCE 1
#define GW_ACCOUNT_PUBKEY_HASH 2
#define GW_ACCOUNT_CODE_HASH 3

#include "blake2b.h"
#include "blockchain.h"
#include "ckb_dlfcn.h"
#include "godwoken.h"
#include "gw_def.h"
#include "stddef.h"

/* common functions */

/* Generate raw key
 * raw_key: blake2b(id | type | key)
 *
 * We use raw key in the underlying KV store
 */
void gw_build_raw_key(uint32_t id, const uint8_t key[GW_KEY_BYTES],
                      uint8_t raw_key[GW_KEY_BYTES]) {
  uint8_t type = GW_ACCOUNT_KV;
  blake2b_state blake2b_ctx;
  blake2b_init(&blake2b_ctx, GW_KEY_BYTES);
  blake2b_update(&blake2b_ctx, (uint8_t *)&id, 4);
  blake2b_update(&blake2b_ctx, (uint8_t *)&type, 1);
  blake2b_update(&blake2b_ctx, key, GW_KEY_BYTES);
  blake2b_final(&blake2b_ctx, raw_key, GW_KEY_BYTES);
}

int gw_get_call_type(gw_context_t *ctx, uint8_t *call_type) {
  mol_seg_t call_context_seg;
  call_context_seg.ptr = ctx->call_context;
  call_context_seg.size = ctx->call_context_len;
  mol_seg_t call_type_seg =
      MolReader_CallContext_get_call_type(&call_context_seg);
  if (call_type_seg.size != 1) {
    return GW_ERROR_INVALID_DATA;
  }
  *call_type = *(uint8_t *)(call_type_seg.ptr);
  return 0;
}

int gw_get_account_id(gw_context_t *ctx, uint32_t *id) {
  mol_seg_t call_context_seg;
  call_context_seg.ptr = ctx->call_context;
  call_context_seg.size = ctx->call_context_len;
  mol_seg_t account_id_seg = MolReader_CallContext_get_to_id(&call_context_seg);
  if (account_id_seg.size != 4) {
    return GW_ERROR_INVALID_DATA;
  }
  *id = *(uint32_t *)(account_id_seg.ptr);
  return 0;
}

int load_layer2_contract_from_args(uint8_t *code_buffer, uint32_t buffer_size,
                                   void *handle) {
  size_t len;
  int ret;
  /* dynamic load contract */
  uint8_t script[SCRIPT_SIZE];
  len = SCRIPT_SIZE;
  ret = ckb_load_script(script, &len, 0);
  if (ret != CKB_SUCCESS) {
    return ret;
  }
  if (len > SCRIPT_SIZE) {
    return GW_ERROR_INVALID_DATA;
  }
  mol_seg_t script_seg;
  script_seg.ptr = script;
  script_seg.size = len;
  if (MolReader_Script_verify(&script_seg, false) != MOL_OK) {
    return GW_ERROR_INVALID_DATA;
  }

  mol_seg_t args_seg = MolReader_Script_get_args(&script_seg);
  mol_seg_t code_hash_seg = MolReader_Bytes_raw_bytes(&args_seg);

  if (code_hash_seg.size != 32) {
    return GW_ERROR_INVALID_DATA;
  }

  uint64_t consumed_size = 0;
  ret = ckb_dlopen(code_hash_seg.ptr, code_buffer, buffer_size, &handle,
                   &consumed_size);
  if (ret != CKB_SUCCESS) {
    return ret;
  }
  if (consumed_size > buffer_size) {
    return GW_ERROR_INVALID_DATA;
  }

  return 0;
}
