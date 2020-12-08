#ifndef GW_COMMON_H_
#define GW_COMMON_H_

/* Common parameters */
#define MAX_PAIRS 1024
#define SCRIPT_SIZE 128
#define WITNESS_SIZE (300 * 1024)
#define CODE_SIZE (512 * 1024)

/* Errors */
#define GW_ERROR_NOT_FOUND 42
#define GW_ERROR_INVALID_DATA 43
#define GW_ERROR_INSUFFICIENT_CAPACITY 44
#define GW_ERROR_INVALID_CONTEXT 45
#define GW_ERROR_DYNAMIC_LINKING 46
#define GW_ERROR_MISMATCH_CHANGE_SET 47
#define GW_ERROR_MISMATCH_RETURN_DATA 48
/*Merkle Errors*/
#define GW_ERROR_INVALID_PROOF_LENGTH 60
#define GW_ERROR_INVALID_PROOF 61
#define GW_ERROR_INVALID_STACK 62
#define GW_ERROR_INVALID_SIBLING 63

#include "blake2b.h"
#include "blockchain.h"
#include "godwoken.h"
#include "gw_def.h"
#include "stddef.h"

typedef unsigned __int128 uint128_t;

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
void gw_build_account_key(uint32_t id, const uint8_t key[GW_KEY_BYTES],
                          uint8_t raw_key[GW_KEY_BYTES]) {
  uint8_t type = GW_ACCOUNT_KV;
  blake2b_state blake2b_ctx;
  blake2b_init(&blake2b_ctx, GW_KEY_BYTES);
  blake2b_update(&blake2b_ctx, (uint8_t *)&id, 4);
  blake2b_update(&blake2b_ctx, (uint8_t *)&type, 1);
  blake2b_update(&blake2b_ctx, key, GW_KEY_BYTES);
  blake2b_final(&blake2b_ctx, raw_key, GW_KEY_BYTES);
}

void gw_build_code_hash_key(uint32_t id, uint8_t key[GW_KEY_BYTES]) {
  memset(key, 0, GW_KEY_BYTES);
  memcpy(key, (uint8_t *)&id, sizeof(uint32_t));
  key[sizeof(uint32_t)] = GW_ACCOUNT_CODE_HASH;
}

int gw_parse_transaction_context(gw_transaction_context_t *transaction_context,
                                 mol_seg_t *src) {
  if (MolReader_RawL2Transaction_verify(src, false) != MOL_OK) {
    return GW_ERROR_INVALID_DATA;
  }
  mol_seg_t from_id_seg = MolReader_RawL2Transaction_get_from_id(src);
  mol_seg_t to_id_seg = MolReader_RawL2Transaction_get_to_id(src);
  mol_seg_t args_bytes_seg = MolReader_RawL2Transaction_get_args(src);
  mol_seg_t args_seg = MolReader_Bytes_raw_bytes(&args_bytes_seg);
  transaction_context->from_id = *(uint32_t *)from_id_seg.ptr;
  transaction_context->to_id = *(uint32_t *)to_id_seg.ptr;
  transaction_context->args = args_seg.ptr;
  transaction_context->args_len = args_seg.size;
  return 0;
}

int gw_parse_block_info(gw_block_info_t *block_info, mol_seg_t *src) {
  if (MolReader_BlockInfo_verify(src, false) != MOL_OK) {
    return GW_ERROR_INVALID_DATA;
  }
  mol_seg_t number_seg = MolReader_BlockInfo_get_number(src);
  mol_seg_t timestamp_seg = MolReader_BlockInfo_get_timestamp(src);
  mol_seg_t aggregator_id_seg = MolReader_BlockInfo_get_aggregator_id(src);
  block_info->number = *(uint64_t *)number_seg.ptr;
  block_info->timestamp = *(uint64_t *)timestamp_seg.ptr;
  block_info->aggregator_id = *(uint32_t *)aggregator_id_seg.ptr;
  return 0;
}

#endif /* GW_COMMON_H_ */