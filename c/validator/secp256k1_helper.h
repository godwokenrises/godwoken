#ifndef CKB_SECP256K1_HELPER_H_
#define CKB_SECP256K1_HELPER_H_

#include "ckb_syscalls.h"
#include "secp256k1_data_info.h"

#define CKB_SECP256K1_HELPER_ERROR_LOADING_DATA -101
#define CKB_SECP256K1_HELPER_ERROR_ILLEGAL_CALLBACK -102
#define CKB_SECP256K1_HELPER_ERROR_ERROR_CALLBACK -103

/*
 * We are including secp256k1 implementation directly so gcc can strip
 * unused functions. For some unknown reasons, if we link in libsecp256k1.a
 * directly, the final binary will include all functions rather than those used.
 */
#define HAVE_CONFIG_H 1
#define USE_EXTERNAL_DEFAULT_CALLBACKS
#include <secp256k1.c>

void secp256k1_default_illegal_callback_fn(const char* str, void* data) {
  (void)str;
  (void)data;
  ckb_exit(CKB_SECP256K1_HELPER_ERROR_ILLEGAL_CALLBACK);
}

void secp256k1_default_error_callback_fn(const char* str, void* data) {
  (void)str;
  (void)data;
  ckb_exit(CKB_SECP256K1_HELPER_ERROR_ERROR_CALLBACK);
}

/*
 * data should at least be CKB_SECP256K1_DATA_SIZE big
 * so as to hold all loaded data.
 */
int ckb_secp256k1_custom_verify_only_initialize(secp256k1_context* context,
                                                void* data) {
  size_t index = 0;
  int running = 1;
  while (running && index < SIZE_MAX) {
    uint64_t len = 32;
    uint8_t hash[32];

    int ret = ckb_load_cell_by_field(hash, &len, 0, index, CKB_SOURCE_CELL_DEP,
                                     CKB_CELL_FIELD_DATA_HASH);
    switch (ret) {
      case CKB_ITEM_MISSING:
        break;
      case CKB_SUCCESS:
        if (memcmp(ckb_secp256k1_data_hash, hash, 32) == 0) {
          /* Found a match, load data here */
          len = CKB_SECP256K1_DATA_SIZE;
          ret = ckb_load_cell_data(data, &len, 0, index, CKB_SOURCE_CELL_DEP);
          if (ret != CKB_SUCCESS || len != CKB_SECP256K1_DATA_SIZE) {
            return CKB_SECP256K1_HELPER_ERROR_LOADING_DATA;
          }
          running = 0;
        }
        break;
      default:
        return CKB_SECP256K1_HELPER_ERROR_LOADING_DATA;
    }
    if (running) {
      index++;
    }
  }
  if (index == SIZE_MAX) {
    return CKB_SECP256K1_HELPER_ERROR_LOADING_DATA;
  }

  context->illegal_callback = default_illegal_callback;
  context->error_callback = default_error_callback;

  secp256k1_ecmult_context_init(&context->ecmult_ctx);
  secp256k1_ecmult_gen_context_init(&context->ecmult_gen_ctx);

  /* Recasting data to (uint8_t*) for pointer math */
  uint8_t* p = (uint8_t*)data;
  secp256k1_ge_storage(*pre_g)[] = (secp256k1_ge_storage(*)[])p;
  secp256k1_ge_storage(*pre_g_128)[] =
      (secp256k1_ge_storage(*)[])(&p[CKB_SECP256K1_DATA_PRE_SIZE]);
  context->ecmult_ctx.pre_g = pre_g;
  context->ecmult_ctx.pre_g_128 = pre_g_128;

  return 0;
}

#endif
