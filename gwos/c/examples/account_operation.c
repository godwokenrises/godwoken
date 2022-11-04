
/*
 * This is a layer2 contract example demonstrate account operations. And also for test purpose.
 *
 *  Include operations:
 *   - sys_load(account_id, key)
 *   - sys_store(account_id, key, value)
 *   - sys_get_account_nonce(account_id)
 *   - sys_log(account_id, service_flag, data)
 */

#include "ckb_syscalls.h"
#include "gw_syscalls.h"
#include "stdio.h"

#define FLAG_SYS_LOAD 0xF0
#define FLAG_SYS_STORE 0xF1
#define FLAG_SYS_LOAD_NONCE 0xF2
#define FLAG_SYS_LOG 0xF3

typedef int (*handler_fn) (gw_context_t *ctx,
                           const uint8_t *args, const uint32_t args_len,
                           uint32_t *rv_len, uint8_t *rv);

int handle_sys_load(gw_context_t *ctx,
                    const uint8_t *args, const uint32_t args_len,
                    uint32_t *rv_len, uint8_t *rv) {
  if (args_len < 4 + 32) {
    printf("invalid args length for sys_load");
    return -1;
  }
  uint32_t account_id = *((uint32_t *)args);
  uint8_t key[32] = {0};
  memcpy(key, args + 4, 32);
  int ret = ctx->sys_load(ctx, account_id, key, 32, rv);
  if (ret != 0) {
    return ret;
  }
  *rv_len = 32;
  return 0;
}
int handle_sys_store(gw_context_t *ctx,
                     const uint8_t *args, const uint32_t args_len,
                     uint32_t *rv_len, uint8_t *rv) {
  if (args_len < 4 + 32 + 32) {
    printf("invalid args length for sys_store");
    return -1;
  }
  uint32_t account_id = *((uint32_t *)args);
  uint8_t key[32] = {0};
  uint8_t value[32] = {0};
  memcpy(key, args + 4, 32);
  memcpy(value, args + 4 + 32, 32);
  int ret = ctx->sys_store(ctx, account_id, key, 32, value);
  if (ret != 0) {
    return ret;
  }
  *rv_len = 0;
  return 0;
}
int handle_sys_load_nonce(gw_context_t *ctx,
                          const uint8_t *args, const uint32_t args_len,
                          uint32_t *rv_len, uint8_t *rv) {
  if (args_len < 4) {
    printf("invalid args length for sys_get_account_nonce");
    return -1;
  }
  uint32_t account_id = *((uint32_t *)args);
  uint32_t nonce = 0;
  int ret = ctx->sys_get_account_nonce(ctx, account_id, &nonce);
  if (ret != 0) {
    return ret;
  }
  memcpy(rv, (uint8_t *)&nonce, 4);
  *rv_len = 4;
  return 0;
}
int handle_sys_log(gw_context_t *ctx,
                   const uint8_t *args, const uint32_t args_len,
                   uint32_t *rv_len, uint8_t *rv) {
  if (args_len < 4 + 1 + 4) {
    printf("invalid args length for sys_log (header)");
    return -1;
  }
  uint32_t account_id = *((uint32_t *)args);
  uint8_t service_flag = args[4];
  uint32_t data_len = *((uint32_t *)(args + 5));
  if (args_len < data_len + 9) {
    printf("invalid args length for sys_log (data part)");
    return -1;
  }
  const uint8_t *data = args + 9;
  int ret = ctx->sys_log(ctx, account_id, service_flag, data_len, data);
  if (ret != 0) {
    printf("call sys_log failed");
    return ret;
  }
  *rv_len = 0;
  return 0;
}

int main() {
  int ret;
  gw_context_t ctx = {0};
  ret = gw_context_init(&ctx);
  if (ret != 0) {
    return ret;
  }
  uint8_t flag = ctx.transaction_context.args[0];
  uint8_t *args = ctx.transaction_context.args + 1;
  uint32_t args_len = ctx.transaction_context.args_len - 1;
  handler_fn handler = NULL;
  switch (flag) {
  case FLAG_SYS_LOAD:
    handler = handle_sys_load;
    break;
  case FLAG_SYS_STORE:
    handler = handle_sys_store;
    break;
  case FLAG_SYS_LOAD_NONCE:
    handler = handle_sys_load_nonce;
    break;
  case FLAG_SYS_LOG:
    handler = handle_sys_log;
    break;
  default:
    printf("invalid flag");
    return -1;
  }
  uint8_t rv[64 * 1024];
  uint32_t rv_len = 0;
  ret = handler(&ctx, args, args_len, &rv_len, rv);
  if (ret != 0) {
    printf("call handler failed");
    return ret;
  }
  ret = ctx.sys_set_program_return_data(&ctx, rv, rv_len);
  if (ret != 0) {
    printf("set return data failed");
    return ret;
  }
  return gw_finalize(&ctx);
}
