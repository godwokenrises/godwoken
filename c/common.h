#ifndef GW_COMMON_H_
#define GW_COMMON_H_

/* Layer2 contract interface */
#define CONTRACT_CONSTRUCTOR_FUNC "gw_construct"
#define CONTRACT_HANDLE_FUNC "gw_handle_message"

/* Common parameters */
#define MAX_PAIRS 1024
#define SCRIPT_SIZE 128
#define WITNESS_SIZE (300 * 1024)
#define CODE_SIZE (512 * 1024)
#define MAX_RETURN_DATA_SIZE 1024

/* Errors */
#define GW_ERROR_NOT_FOUND 42
#define GW_ERROR_INVALID_DATA 43
#define GW_ERROR_INSUFFICIENT_CAPACITY 44
#define GW_ERROR_INVALID_CONTEXT 45
#define GW_ERROR_DYNAMIC_LINKING 46
#define GW_ERROR_MISMATCH_CHANGE_SET 47
#define GW_ERROR_MISMATCH_RETURN_DATA 48

/* Key type */
#define GW_ACCOUNT_KV 0
#define GW_ACCOUNT_NONCE 1
#define GW_ACCOUNT_PUBKEY_HASH 2
#define GW_ACCOUNT_CODE_HASH 3

#include "blake2b.h"
#include "blockchain.h"
#include "godwoken.h"
#include "gw_def.h"
#include "stddef.h"

/* common functions */

/* Implement of gw_blake2b_hash_fn
 * Note: this function is used in layer2 contract
 */
void blake2b_hash(uint8_t output_hash[GW_KEY_BYTES], uint8_t *input_data,
                  uint32_t len) {
  blake2b_state blake2b_ctx;
  blake2b_init(&blake2b_ctx, GW_KEY_BYTES);
  blake2b_update(&blake2b_ctx, input_data, len);
  blake2b_final(&blake2b_ctx, output_hash, GW_KEY_BYTES);
}

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

#endif /* GW_COMMON_H_ */