/* Layer2 contract interface */
#define CONTRACT_CONSTRUCTOR_FUNC "construct"
#define CONTRACT_HANDLE_FUNC "handle_message"

/* 4 bytes id + 1 byte + 32 bytes key */
#define GW_KEY_BYTES 37
#define GW_VALUE_BYTES 32

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

#include "blockchain.h"
#include "ckb_dlfcn.h"
#include "godwoken.h"
#include "stddef.h"

/* layer2 syscalls */
typedef int (*sys_load_fn)(void *ctx, const uint8_t key[GW_KEY_BYTES],
                           uint8_t value[GW_VALUE_BYTES]);
typedef int (*sys_store_fn)(void *ctx, const uint8_t key[GW_KEY_BYTES],
                            const uint8_t value[GW_VALUE_BYTES]);

/* Godwoken context */
typedef struct {
  /* verification context */
  uint8_t *call_context;
  uint32_t call_context_len;
  uint8_t *block_info;
  uint32_t block_info_len;
  /* layer2 syscalls */
  void *sys_context;
  sys_load_fn sys_load;
  sys_store_fn sys_store;
} gw_context_t;

/* layer2 contract interfaces */
typedef int (*contract_handle_fn)(gw_context_t *);

/* common functions */
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
