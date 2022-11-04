#include "gw_syscalls.h"
#include "sudt_utils.h"

#define ERROR_INVALID_SUDT_ID 99

/* transtions's args should be a uint32_t sudt id */
int main() {
  gw_context_t ctx = {0};
  int ret = gw_context_init(&ctx);
  if (ret != 0) {
    return ret;
  }

  // extract sudt id
  uint32_t sudt_id = 0;
  if (sizeof(uint32_t) != ctx.transaction_context.args_len) {
    return ERROR_INVALID_SUDT_ID;
  }
  sudt_id = *(uint32_t *)ctx.transaction_context.args;

  uint256_t total_supply = {0};
  ret = sudt_get_total_supply(&ctx, sudt_id, &total_supply);
  if (ret != 0) {
    return ret;
  }

  ctx.sys_set_program_return_data(&ctx, (uint8_t *)&total_supply, 32);
  return gw_finalize(&ctx);
}
