#define GW_KEY_BYTES 32
#define GW_VALUE_BYTES 32

/* layer2 syscalls */
typedef int (*gw_sys_load_fn)(void *ctx, const uint8_t key[GW_KEY_BYTES],
                              uint8_t value[GW_VALUE_BYTES]);
typedef int (*gw_sys_store_fn)(void *ctx, const uint8_t key[GW_KEY_BYTES],
                               const uint8_t value[GW_VALUE_BYTES]);
typedef int (*gw_set_return_data_fn)(void *ctx, uint8_t *data, uint32_t len);

/* Blake2b hash function wrapper */
typedef void (*gw_blake2b_hash_fn)(uint8_t output_hash[GW_KEY_BYTES],
                                   uint8_t *input_data, uint32_t len);

/* Godwoken context */
typedef struct {
  /* verification context */
  uint8_t *call_context;
  uint32_t call_context_len;
  uint8_t *block_info;
  uint32_t block_info_len;
  /* layer2 syscalls */
  void *sys_context;
  gw_sys_load_fn sys_load;
  gw_sys_store_fn sys_store;
  gw_set_return_data_fn sys_set_return_data;
  /* blake2b hash function helper */
  gw_blake2b_hash_fn blake2b_hash;
} gw_context_t;

/* layer2 contract interfaces */
typedef int (*gw_contract_fn)(gw_context_t *);
