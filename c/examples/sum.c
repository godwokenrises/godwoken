/*
 * The Sum is a layer2 contract example.
 *
 * The Godwoken layer2 contract must be implemented as a shared library.
 * We expose two interfaces as the requirement asked:
 * - `gw_construct`: contract constructor, be invoked when the contract created
 * - `gw_handle_message`: call handler, invoked when a transaction/message send
 * to this contract
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
  uint64_t counter_value = 0;
  ret = read_counter(&ctx, &counter_value);
  if (ret != 0) {
    return ret;
  }
  uint64_t add_value = 0;
  ret = extract_args(&ctx, &add_value);
  if (ret != 0) {
    return ret;
  }
  counter_value = saturating_add(counter_value, add_value);
  /* return current counter value as data */
  ctx.sys_set_program_return_data(&ctx, (uint8_t *)&counter_value,
                                  sizeof(uint64_t));
  return write_counter(&ctx, counter_value);
}

/* helper functions */

uint64_t saturating_add(uint64_t a, uint64_t b) {
  uint64_t c = a + b;
  if (c < a) {
    c = -1;
  }
  return c;
}

int extract_args(gw_context_t *ctx, uint64_t *v) {
  if (sizeof(uint64_t) != ctx->transaction_context.args_len) {
    return ERROR_INVALID_DATA;
  }
  *v = *(uint64_t *)ctx->transaction_context.args;
  return 0;
}

int read_counter(gw_context_t *ctx, uint64_t *counter) {
  uint8_t key[GW_KEY_BYTES];
  blake2b_hash(key, (uint8_t *)"counter", 7);
  uint8_t value[GW_VALUE_BYTES];
  int ret = ctx->sys_load(ctx, ctx->transaction_context.to_id, key, value);
  if (ret != 0) {
    return ret;
  }
  *counter = *(uint64_t *)value;
  return 0;
}

int write_counter(gw_context_t *ctx, uint64_t counter) {
  uint8_t key[GW_KEY_BYTES];
  blake2b_hash(key, (uint8_t *)"counter", 7);
  uint8_t value[GW_VALUE_BYTES];
  *(uint64_t *)value = counter;
  return ctx->sys_store(ctx, ctx->transaction_context.to_id, key, value);
}
