/*
 * SUDT Utils
 * Godwoken backend use this utils to modify SUDT states from the SMT.
 */

#include "godwoken.h"
#include "gw_def.h"
#include "stdio.h"

/* errors */
#define ERROR_INVALID_DATA 10
#define ERROR_UNKNOWN_MSG 11
#define ERROR_INSUFFICIENT_BALANCE 12
#define ERROR_AMOUNT_OVERFLOW 13

/* Prepare withdrawal fields */
#define WITHDRAWAL_LOCK_HASH 1
#define WITHDRAWAL_AMOUNT 2
#define WITHDRAWAL_BLOCK_NUMBER 3

void _sudt_id_to_key(const uint32_t account_id, uint8_t key[32]) {
  memcpy(key, (uint8_t *)&account_id, 4);
}

int _sudt_get_balance(gw_context_t *ctx, uint32_t sudt_id,
                      const uint8_t key[32], uint128_t *balance) {
  uint8_t value[32] = {0};
  int ret = ctx->sys_load(ctx, sudt_id, key, value);
  if (ret != 0) {
    return ret;
  }
  *balance = *(uint128_t *)value;
  return 0;
}

int _sudt_set_balance(gw_context_t *ctx, uint32_t sudt_id, uint8_t key[32],
                      uint128_t balance) {
  uint8_t value[32] = {0};
  *(uint128_t *)value = balance;
  int ret = ctx->sys_store(ctx, sudt_id, key, value);
  return ret;
}

int sudt_get_balance(gw_context_t *ctx, uint32_t sudt_id, uint32_t account_id,
                     uint128_t *balance) {
  uint8_t key[32] = {0};
  _sudt_id_to_key(account_id, key);
  return _sudt_get_balance(ctx, sudt_id, key, balance);
}

/* Transfer Simple UDT */
int sudt_transfer(gw_context_t *ctx, uint32_t sudt_id, uint32_t from_id,
                  uint32_t to_id, uint128_t amount) {
  /* check from account */
  uint8_t from_key[32] = {0};
  _sudt_id_to_key(from_id, from_key);
  uint128_t from_balance;
  int ret = _sudt_get_balance(ctx, sudt_id, from_key, &from_balance);
  if (ret != 0) {
    return ret;
  }
  if (from_balance < amount) {
    return ERROR_INSUFFICIENT_BALANCE;
  }
  uint128_t new_from_balance = from_balance - amount;

  /* check to account */
  uint8_t to_key[32] = {0};
  _sudt_id_to_key(to_id, to_key);
  uint128_t to_balance;
  ret = _sudt_get_balance(ctx, sudt_id, to_key, &to_balance);
  if (ret != 0) {
    return ret;
  }
  uint128_t new_to_balance = to_balance + amount;
  if (new_to_balance < to_balance) {
    return ERROR_AMOUNT_OVERFLOW;
  }

  /* update balance */
  ret = _sudt_set_balance(ctx, sudt_id, from_key, new_from_balance);
  if (ret != 0) {
    return ret;
  }
  return _sudt_set_balance(ctx, sudt_id, to_key, new_to_balance);
}

int sudt_prepare_withdrawal(gw_context_t *ctx,
                       const uint8_t withdrawal_lock_hash[32],
                       uint128_t amount) {
  /* store prepare withdrawal (account_id, block_number, withdrawal_lock_hash,
   * amount) */
  return 0;
}
