#define CONTRACT_CONSTRUCTOR_FUNC "construct"
#define CONTRACT_HANDLE_FUNC "handle_message"
/* 4 bytes id + 1 byte + 32 bytes key */
#define GW_KEY_BYTES 37
#define GW_VALUE_BYTES 32

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
