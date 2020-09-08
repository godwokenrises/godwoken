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
#include "gw_smt.h"
#include "stdlib.h"
#include "string.h"

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
  gw_call_receipt_t *receipt;
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
  uint32_t account_id = gw_ctx->call_context.to_id;
  /* raw key */
  uint8_t raw_key[GW_KEY_BYTES];
  gw_build_raw_key(account_id, key, raw_key);
  /* try read from write_state
   * if not found then read from read_state */
  int ret = gw_state_fetch(state->write_state, raw_key, value);
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
  uint32_t account_id = gw_ctx->call_context.to_id;
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
  if (len > GW_MAX_RETURN_DATA_SIZE) {
    return GW_ERROR_INSUFFICIENT_CAPACITY;
  }
  state->receipt->return_data_len = len;
  memcpy(state->receipt->return_data, data, len);
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

  /* merkle verify pre_account_state */
  mol_seg_t prev_account_state_seg =
      MolReader_VerificationContext_get_prev_account_state(
          &verification_context_seg);
  mol_seg_t inputs_seg =
      MolReader_VerificationContext_get_inputs(&verification_context_seg);

  gw_pair_t read_pairs[MAX_PAIRS];
  gw_state_t read_state;
  gw_state_init(&read_state, read_pairs, MAX_PAIRS);
  ret = gw_state_load_from_state_set(&read_state, &inputs_seg);
  if (ret != 0) {
    return ret;
  }

  mol_seg_t proof_bytes_seg =
      MolReader_VerificationContext_get_proof(&verification_context_seg);
  mol_seg_t proof_seg = MolReader_Bytes_raw_bytes(&proof_bytes_seg);

  ret = gw_smt_verify(prev_account_state_seg.ptr, &read_state, proof_seg.ptr,
                      proof_seg.size);

  if (ret != 0) {
    return ret;
  }

  /* prepare context */
  gw_pair_t write_pairs[MAX_PAIRS];
  gw_state_t write_state;
  gw_state_init(&write_state, write_pairs, MAX_PAIRS);

  gw_call_receipt_t receipt;
  gw_read_write_state_t state;
  state.read_state = &read_state;
  state.write_state = &write_state;
  state.receipt = &receipt;

  gw_context_t context;
  context.blake2b_hash = blake2b_hash;
  context.sys_context = (void *)&state;
  context.sys_load = sys_load;
  context.sys_store = sys_store;
  context.sys_set_return_data = sys_set_return_data;
  ret = gw_parse_call_context(&context.call_context, &call_context_seg);
  if (ret != 0) {
    return ret;
  }
  ret = gw_parse_block_info(&context.block_info, &block_info_seg);
  if (ret != 0) {
    return ret;
  }

  /* get contract function pointer */
  uint8_t call_type = context.call_context.call_type;

  char *func_name;
  ret = gw_get_func_name_by_call_type(&func_name, call_type);
  if (ret != 0) {
    return ret;
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

  /* merkle verify post account state */
  gw_state_normalize(state.write_state);
  mol_seg_t post_account_state_seg =
      MolReader_VerificationContext_get_post_account_state(
          &verification_context_seg);
  ret = gw_smt_verify(post_account_state_seg.ptr, state.write_state,
                      proof_seg.ptr, proof_seg.size);

  if (ret != 0) {
    return ret;
  }

  /* verify return_data */
  uint8_t return_data_hash_buffer[32];
  blake2b_state blake2b_ctx;
  blake2b_init(&blake2b_ctx, 32);
  blake2b_update(&blake2b_ctx, state.receipt->return_data,
                 state.receipt->return_data_len);
  blake2b_final(&blake2b_ctx, return_data_hash_buffer, 32);

  mol_seg_t return_data_hash_seg =
      MolReader_VerificationContext_get_return_data_hash(
          &verification_context_seg);
  if (memcmp(return_data_hash_seg.ptr, return_data_hash_buffer,
             return_data_hash_seg.size) != 0) {
    return GW_ERROR_MISMATCH_RETURN_DATA;
  }

  return 0;
}
