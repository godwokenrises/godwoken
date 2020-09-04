/* Layer2 contract validator
 *
 * Validator provides on-chain layer2 syscall implementation for contracts.
 * The `code_hash` of the layer2 contract is required in the args.
 * A cell that matches the `code_hash` should be put into the transactions's
 * `cell_deps` field, Validator dynamic linking with the code; setup the layer2
 * verification context; then call the contract.
 *
 * Args: <code_hash> layer2 contract's code hash
 *
 *  1. load contract
 *  2. load verification context from witness
 *  3. run contract (injection verification context and syscalls)
 *  4. compare actual outputs with expected outputs
 */

#include "ckb_dlfcn.h"
#include "ckb_syscalls.h"
#include "common.h"
#include "stdlib.h"
#include "string.h"

typedef struct {
  uint8_t key[GW_KEY_BYTES];
  uint8_t value[GW_VALUE_BYTES];
  uint8_t order;
} gw_pair_t;

typedef struct {
  gw_pair_t *pairs;
  uint32_t len;
  uint32_t capacity;
} gw_state_t;

void gw_state_init(gw_state_t *state, gw_pair_t *buffer, uint32_t capacity) {
  state->pairs = buffer;
  state->len = 0;
  state->capacity = capacity;
}

int gw_state_insert(gw_state_t *state, const uint8_t key[GW_KEY_BYTES],
                    const uint8_t value[GW_VALUE_BYTES]) {
  if (state->len < state->capacity) {
    /* shortcut, append at end */
    memcpy(state->pairs[state->len].key, key, GW_KEY_BYTES);
    memcpy(state->pairs[state->len].value, value, GW_KEY_BYTES);
    state->len++;
    return 0;
  }

  /* Find a matched key and overwritten it */
  int32_t i = state->len - 1;
  for (; i >= 0; i--) {
    if (memcmp(key, state->pairs[i].key, GW_KEY_BYTES) == 0) {
      break;
    }
  }

  if (i < 0) {
    return GW_ERROR_INSUFFICIENT_CAPACITY;
  }

  memcpy(state->pairs[i].value, value, GW_VALUE_BYTES);
  return 0;
}

int gw_state_fetch(gw_state_t *state, const uint8_t key[GW_KEY_BYTES],
                   uint8_t value[GW_VALUE_BYTES]) {
  int32_t i = state->len - 1;
  for (; i >= 0; i--) {
    if (memcmp(key, state->pairs[i].key, GW_KEY_BYTES) == 0) {
      memcpy(value, state->pairs[i].value, GW_VALUE_BYTES);
      return 0;
    }
  }
  return GW_ERROR_NOT_FOUND;
}

int _gw_pair_cmp(const void *a, const void *b) {
  const gw_pair_t *pa = (const gw_pair_t *)a;
  const gw_pair_t *pb = (const gw_pair_t *)b;

  for (uint32_t i = GW_KEY_BYTES - 1; i >= 0; i--) {
    int cmp_result = pa->key[i] - pb->key[i];
    if (cmp_result != 0) {
      return cmp_result;
    }
  }
  return pa->order - pb->order;
}

void gw_state_normalize(gw_state_t *state) {
  for (uint32_t i = 0; i < state->len; i++) {
    state->pairs[i].order = i;
  }
  qsort(state->pairs, state->len, sizeof(gw_pair_t), _gw_pair_cmp);
  /* Remove duplicate ones */
  int32_t sorted = 0, next = 0;
  while (next < state->len) {
    int32_t item_index = next++;
    while (next < state->len &&
           memcmp(state->pairs[item_index].key, state->pairs[next].key,
                  GW_KEY_BYTES) == 0) {
      next++;
    }
    if (item_index != sorted) {
      memcpy(state->pairs[sorted].key, state->pairs[item_index].key,
             GW_KEY_BYTES);
      memcpy(state->pairs[sorted].value, state->pairs[item_index].value,
             GW_VALUE_BYTES);
    }
    sorted++;
  }
  state->len = sorted;
}

/* return 0 if state is equal, otherwise return non-zero value */
int gw_cmp_state(gw_state_t *state_a, gw_state_t *state_b) {
  if (state_a->len != state_b->len) {
    return -1;
  }

  for (uint32_t i = 0; i < state_a->len; i++) {
    gw_pair_t *a = &(state_a->pairs[i]);
    gw_pair_t *b = &(state_b->pairs[i]);

    if (memcmp(a->key, b->key, GW_KEY_BYTES) != 0) {
      return -1;
    }

    if (memcmp(a->value, b->value, GW_VALUE_BYTES) != 0) {
      return -1;
    }
  }

  return 0;
}

/* load state from inputs_seg */
int gw_state_load_from_state_set(gw_state_t *state, mol_seg_t *inputs_seg) {
  uint32_t len = MolReader_KVPairVec_length(inputs_seg);
  for (uint32_t i = 0; i < len; i++) {
    mol_seg_res_t kv_pair_res = MolReader_KVPairVec_get(inputs_seg, i);
    if (kv_pair_res.errno != MOL_OK) {
      return GW_ERROR_INVALID_DATA;
    }
    mol_seg_t k_seg = MolReader_KVPair_get_k(&kv_pair_res.seg);
    mol_seg_t v_seg = MolReader_KVPair_get_v(&kv_pair_res.seg);
    int ret = gw_state_insert(state, k_seg.ptr, v_seg.ptr);
    if (ret != 0) {
      return ret;
    }
  }
  return 0;
}

/* syscalls */

typedef struct {
  gw_state_t *read_state;
  gw_state_t *write_state;
  uint8_t return_data[MAX_RETURN_DATA_SIZE];
  uint32_t return_data_len;
} gw_read_write_state_t;

int sys_load(void *ctx, const uint8_t key[GW_KEY_BYTES],
             uint8_t value[GW_VALUE_BYTES]) {
  if (ctx == NULL) {
    return GW_ERROR_INVALID_CONTEXT;
  }

  gw_context_t *gw_ctx = (gw_context_t *)ctx;
  gw_read_write_state_t *state = (gw_read_write_state_t *)gw_ctx->sys_context;
  if (state == NULL) {
    return GW_ERROR_INVALID_CONTEXT;
  }
  /* get account id */
  uint32_t account_id;
  int ret = gw_get_account_id(gw_ctx, &account_id);
  if (ret != 0) {
    return ret;
  }
  /* raw key */
  uint8_t raw_key[GW_KEY_BYTES];
  gw_build_raw_key(account_id, key, raw_key);
  /* try read from write_state
   * if not found then read from read_state */
  ret = gw_state_fetch(state->write_state, raw_key, value);
  if (ret == GW_ERROR_NOT_FOUND) {
    ret = gw_state_fetch(state->read_state, raw_key, value);
  }
  return ret;
}

int sys_store(void *ctx, const uint8_t key[GW_KEY_BYTES],
              const uint8_t value[GW_VALUE_BYTES]) {
  if (ctx == NULL) {
    return GW_ERROR_INVALID_CONTEXT;
  }
  gw_context_t *gw_ctx = (gw_context_t *)ctx;
  gw_read_write_state_t *state = (gw_read_write_state_t *)gw_ctx->sys_context;
  if (state == NULL) {
    return GW_ERROR_INVALID_CONTEXT;
  }
  /* get account id */
  uint32_t account_id;
  int ret = gw_get_account_id(gw_ctx, &account_id);
  if (ret != 0) {
    return ret;
  }
  /* raw key */
  uint8_t raw_key[GW_KEY_BYTES];
  gw_build_raw_key(account_id, key, raw_key);
  return gw_state_insert(state->write_state, raw_key, value);
}

int sys_set_return_data(void *ctx, uint8_t *data, uint32_t len) {
  if (ctx == NULL) {
    return GW_ERROR_INVALID_CONTEXT;
  }
  gw_context_t *gw_ctx = (gw_context_t *)ctx;
  gw_read_write_state_t *state = (gw_read_write_state_t *)gw_ctx->sys_context;
  if (ctx == NULL) {
    return GW_ERROR_INVALID_CONTEXT;
  }
  if (len > MAX_RETURN_DATA_SIZE) {
    return GW_ERROR_INSUFFICIENT_CAPACITY;
  }
  state->return_data_len = len;
  memcpy(state->return_data, data, len);
  return 0;
}

int load_layer2_code_hash_from_script_args(uint8_t code_hash[32]) {
  size_t len;
  int ret;
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

  memcpy(code_hash, code_hash_seg.ptr, 32);
  return 0;
}

int load_layer2_contract(const uint8_t code_hash[32], uint8_t *code_buffer,
                         uint32_t buffer_size, void *handle) {
  int ret;
  /* dynamic load contract */
  uint64_t consumed_size = 0;
  ret =
      ckb_dlopen(code_hash, code_buffer, buffer_size, &handle, &consumed_size);
  if (ret != CKB_SUCCESS) {
    return ret;
  }
  if (consumed_size > buffer_size) {
    return GW_ERROR_INVALID_DATA;
  }

  return 0;
}

int main() {
  size_t len;
  int ret;

  /* load layer2 contract */
  uint8_t code_hash[32];
  ret = load_layer2_code_hash_from_script_args(code_hash);
  if (ret != 0) {
    return ret;
  }
  uint8_t code_buffer[CODE_SIZE] __attribute__((aligned(RISCV_PGSIZE)));
  void *handle = NULL;
  ret = load_layer2_contract(code_hash, code_buffer, CODE_SIZE, handle);
  if (ret != 0) {
    return ret;
  }

  /* load verification context */
  uint8_t witness[WITNESS_SIZE];
  len = WITNESS_SIZE;
  size_t cell_source = 0;
  ret = ckb_load_actual_type_witness(witness, &len, 0, &cell_source);
  if (ret != CKB_SUCCESS) {
    return ret;
  }
  mol_seg_t witness_seg;
  witness_seg.ptr = (uint8_t *)witness;
  witness_seg.size = len;
  if (MolReader_WitnessArgs_verify(&witness_seg, false) != MOL_OK) {
    return GW_ERROR_INVALID_DATA;
  }
  mol_seg_t content_seg;
  if (cell_source == CKB_SOURCE_GROUP_OUTPUT) {
    content_seg = MolReader_WitnessArgs_get_output_type(&witness_seg);
  } else {
    content_seg = MolReader_WitnessArgs_get_input_type(&witness_seg);
  }
  if (MolReader_BytesOpt_is_none(&content_seg)) {
    return GW_ERROR_INVALID_DATA;
  }
  mol_seg_t verification_context_seg =
      MolReader_Bytes_raw_bytes(&verification_context_seg);
  if (MolReader_VerificationContext_verify(&witness_seg, false) != MOL_OK) {
    return GW_ERROR_INVALID_DATA;
  }
  mol_seg_t call_context_seg =
      MolReader_VerificationContext_get_call_context(&verification_context_seg);
  mol_seg_t block_info_seg =
      MolReader_VerificationContext_get_call_context(&verification_context_seg);

  /* prepare context */
  mol_seg_t inputs_seg =
      MolReader_VerificationContext_get_inputs(&verification_context_seg);

  gw_pair_t read_pairs[MAX_PAIRS];
  gw_state_t read_state;
  gw_state_init(&read_state, read_pairs, MAX_PAIRS);
  ret = gw_state_load_from_state_set(&read_state, &inputs_seg);
  if (ret != 0) {
    return ret;
  }

  gw_pair_t write_pairs[MAX_PAIRS];
  gw_state_t write_state;
  gw_state_init(&write_state, write_pairs, MAX_PAIRS);

  gw_read_write_state_t state;
  state.read_state = &read_state;
  state.write_state = &write_state;

  gw_context_t context;
  context.call_context = call_context_seg.ptr;
  context.call_context_len = call_context_seg.size;
  context.block_info = block_info_seg.ptr;
  context.block_info_len = block_info_seg.size;
  context.blake2b_hash = blake2b_hash;
  context.sys_context = (void *)&state;
  context.sys_load = sys_load;
  context.sys_store = sys_store;
  context.sys_set_return_data = sys_set_return_data;

  /* get contract function pointer */
  uint8_t call_type;
  ret = gw_get_call_type(&context, &call_type);
  if (ret != 0) {
    return ret;
  }

  char *func_name;
  if (call_type == 0) {
    func_name = CONTRACT_CONSTRUCTOR_FUNC;
  } else if (call_type == 1) {
    func_name = CONTRACT_HANDLE_FUNC;
  } else {
    return GW_ERROR_INVALID_DATA;
  }

  gw_contract_fn contract_func;
  *(void **)(&contract_func) = ckb_dlsym(handle, func_name);
  if (contract_func == NULL) {
    return GW_ERROR_DYNAMIC_LINKING;
  }

  /* run contract */
  ret = contract_func(&context);

  if (ret != 0) {
    return ret;
  }

  /* verify outputs */
  gw_state_normalize(state.write_state);

  mol_seg_t changes_seg =
      MolReader_VerificationContext_get_changes(&verification_context_seg);
  gw_state_t change_state;
  /* reuse read_pairs as buffer */
  gw_state_init(&change_state, read_pairs, MAX_PAIRS);
  ret = gw_state_load_from_state_set(&change_state, &changes_seg);
  if (ret != 0) {
    return ret;
  }

  if (gw_cmp_state(state.write_state, &change_state) != 0) {
    return GW_ERROR_MISMATCH_CHANGE_SET;
  }

  /* verify return_data */
  mol_seg_t return_data_bytes_seg =
      MolReader_VerificationContext_get_return_data(&verification_context_seg);
  mol_seg_t return_data_seg = MolReader_Bytes_raw_bytes(&return_data_bytes_seg);
  if (return_data_seg.size != state.return_data_len) {
    return GW_ERROR_MISMATCH_RETURN_DATA;
  }
  if (memcmp(return_data_seg.ptr, state.return_data, state.return_data_len) !=
      0) {
    return GW_ERROR_MISMATCH_RETURN_DATA;
  }

  return 0;
}
