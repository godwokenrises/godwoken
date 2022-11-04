/*
 * The Sum is a layer2 contract example demostrate how to read & write to the
 * account state.
 */

#include "ckb_syscalls.h"
#include "gw_syscalls.h"
#include "stdio.h"

#define ERROR_INVALID_DATA 10

uint64_t saturating_add(uint64_t a, uint64_t b);
int extract_args(gw_context_t *ctx, uint64_t *v);
int read_counter(gw_context_t *ctx, uint64_t *counter);
int write_counter(gw_context_t *ctx, uint64_t counter);

/* transtions's args should be a uint64_t as the accumulate number */
int main() {
  gw_context_t ctx = {0};
  int ret = gw_context_init(&ctx);
  if (ret != 0) {
    return ret;
  }
  uint8_t *args = ctx.transaction_context.args;
  uint32_t args_len = ctx.transaction_context.args_len;
  uint8_t *message = args;
  uint64_t signature_len = (uint64_t)args[32];
  uint8_t *signature = args + 32 + 1;
  uint8_t *code_hash = args + 32 + 1 + signature_len;
  if (args_len != (32 + 1 + signature_len + 32)) {
    printf("invalid args_len");
    return -1;
  }
  uint8_t script[1024] = {0};
  uint64_t script_len = GW_MAX_SCRIPT_SIZE;
  ret = ctx.sys_recover_account(&ctx, message, signature, signature_len,
                                code_hash, script, &script_len);
  if (ret != 0) {
    return ret;
  }

  /* return current counter value as data */
  ctx.sys_set_program_return_data(&ctx, script, script_len);
  return gw_finalize(&ctx);
}
