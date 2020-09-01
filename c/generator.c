/* Layer2 contract generator
 *
 * The generator supposed to be run off-chain.
 * generator dynamic linking with the layer2 contract code,
 * and provides layer2 syscalls.
 *
 * A program should be able to generate a post state after run the generator,
 * and should be able to use the states to construct a transaction that satifies
 * the validator.
 */

#include "ckb_syscalls.h"
#include "common.h"

/* syscalls */
#define GW_SYS_STORE 3051
#define GW_SYS_LOAD 3052
#define GW_SYS_LOAD_CALLCONTEXT 3061
#define GW_SYS_LOAD_BLOCKINFO 3062

#define CALL_CONTEXT_LEN 128
#define BLOCK_INFO_LEN 128

int sys_load(void *ctx, const uint8_t key[GW_KEY_BYTES],
             uint8_t value[GW_VALUE_BYTES]) {
  return syscall(GW_SYS_LOAD, key, value, 0, 0, 0, 0);
}
int sys_store(void *ctx, const uint8_t key[GW_KEY_BYTES],
              const uint8_t value[GW_VALUE_BYTES]) {
  return syscall(GW_SYS_STORE, key, value, 0, 0, 0, 0);
}

int _sys_load_call_context(void *addr, uint64_t *len) {
  volatile uint64_t inner_len = *len;
  int ret = syscall(GW_SYS_LOAD_CALLCONTEXT, addr, &inner_len, 0, 0, 0, 0);
  *len = inner_len;
  return ret;
}

int _sys_load_block_info(void *addr, uint64_t *len) {
  volatile uint64_t inner_len = *len;
  int ret = syscall(GW_SYS_LOAD_BLOCKINFO, addr, &inner_len, 0, 0, 0, 0);
  *len = inner_len;
  return ret;
}

int main() {
  int ret;
  /* load layer2 contract */
  uint8_t code_buffer[CODE_SIZE] __attribute__((aligned(RISCV_PGSIZE)));
  void *handle = NULL;
  ret = load_layer2_contract_from_args(code_buffer, CODE_SIZE, handle);

  if (ret != 0) {
    return ret;
  }

  /* prepare context */
  gw_context_t context;
  context.sys_context = NULL;
  context.sys_load = sys_load;
  context.sys_store = sys_store;

  uint8_t call_context[CALL_CONTEXT_LEN];
  uint64_t len = CALL_CONTEXT_LEN;
  ret = _sys_load_call_context(call_context, &len);
  if (ret != 0) {
    return ret;
  }
  if (len > CALL_CONTEXT_LEN) {
    return GW_ERROR_INVALID_DATA;
  }
  context.call_context = call_context;
  context.call_context_len = len;

  uint8_t block_info[BLOCK_INFO_LEN];
  len = BLOCK_INFO_LEN;
  ret = _sys_load_block_info(block_info, &len);
  if (ret != 0) {
    return ret;
  }
  if (len > BLOCK_INFO_LEN) {
    return GW_ERROR_INVALID_DATA;
  }
  context.block_info = block_info;
  context.block_info_len = len;

  /* get account_id */
  uint8_t call_type;
  ret = gw_get_call_type(&context, &call_type);
  if (ret != 0) {
    return ret;
  }

  /* get contract function pointer */
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

  return 0;
}
