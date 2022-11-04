/*
 * SUDT Utils
 * This file provides Godwoken layer2 Simple UDT related methods.
 *
 * Godwoken backends(Apps) should always operates Simple UDT through methods in
 * this file.
 *
 * ## Terms
 *
 * Simple UDT ID:
 * Simple UDT ID is Godwoken account ID of Simple UDT contract account.
 *
 * Registry:
 * Registry is a concept in the Godwoken, registry mapping Godwoken script hash
 * to native addresses. Such as Ethereum address. Registry itself is a Godwoken
 * contract.
 *
 * ## Storage format
 *
 * Simple UDT is stored in the Godwoken SMT. We represent users balances as KV
 * pairs.
 *
 * The SMT key of user's balance is represent in the following:
 * blake2b(BALANCE_FLAG(value: 1, take 4 bytes) | registry_address)
 *
 * The SMT key of Simple UDT total supply is:
 * 0xffffffffffffffff(32 bytes)
 *
 * To support transfer with backend engine native addresses(such as Ethereum
 * address), we introduce registry address format:
 * `registry_id(4 bytes) | address len (4 bytes) | address(n bytes)`
 */

#include "godwoken.h"
#include "gw_def.h"
#include "gw_registry_addr.h"
#include "gw_syscalls.h"
#include "uint256.h"

#define CKB_SUDT_ACCOUNT_ID 1
#define SUDT_KEY_FLAG_BALANCE 1

const uint8_t SUDT_TOTAL_SUPPLY_KEY[] = {
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
};

/* format:
 * key_flag(4 bytes) | registry_address
 */
int _sudt_build_key(uint32_t key_flag, gw_reg_addr_t registry_address,
                    uint8_t *key, uint32_t *key_len) {
  if (*key_len < (4 + GW_REG_ADDR_SIZE(registry_address))) {
    printf("_sudt_build_key: addr is large than buffer");
    return GW_FATAL_BUFFER_OVERFLOW;
  }
  *key_len = 4 + GW_REG_ADDR_SIZE(registry_address);
  _gw_fast_memcpy(key, (uint8_t *)(&key_flag), 4);
  _gw_cpy_addr(key + 4, registry_address);
  return 0;
}

/* format:
 * from_addr | to_addr | amount(32 bytes)
 */
int _sudt_emit_log(gw_context_t *ctx, const uint32_t sudt_id,
                   gw_reg_addr_t from_addr, gw_reg_addr_t to_addr,
                   const uint256_t amount, uint8_t service_flag) {
#ifdef GW_VALIDATOR
  uint32_t data_size = 0;
  uint8_t *data = NULL;
#else
  uint8_t data[256] = {0};
  /* from_addr + to_addr + amount(32 bytes) */
  uint32_t data_size =
      GW_REG_ADDR_SIZE(from_addr) + GW_REG_ADDR_SIZE(to_addr) + 32;
  if (data_size > 256) {
    printf("_sudt_emit_log: data is large than buffer");
    return GW_FATAL_BUFFER_OVERFLOW;
  }
  _gw_cpy_addr(data, from_addr);
  _gw_cpy_addr(data + GW_REG_ADDR_SIZE(from_addr), to_addr);
  _gw_fast_memcpy(
      data + GW_REG_ADDR_SIZE(from_addr) + GW_REG_ADDR_SIZE(to_addr),
      (uint8_t *)(&amount), 32);
#endif
  return ctx->sys_log(ctx, sudt_id, service_flag, data_size, data);
}

int _sudt_get_balance(gw_context_t *ctx, const uint32_t sudt_id,
                      gw_reg_addr_t address, uint256_t *balance) {
  uint8_t key[64] = {0};
  uint32_t key_len = 64;
  int ret = _sudt_build_key(SUDT_KEY_FLAG_BALANCE, address, key, &key_len);
  if (ret != 0) {
    return ret;
  }
  uint8_t value[32] = {0};
  ret = ctx->sys_load(ctx, sudt_id, key, key_len, value);
  if (ret != 0) {
    return ret;
  }
  _gw_fast_memcpy((uint8_t *)balance, (uint8_t *)(&value), 32);
  return 0;
}

int _sudt_set_balance(gw_context_t *ctx, const uint32_t sudt_id,
                      gw_reg_addr_t address, uint256_t balance) {
  uint8_t key[64] = {0};
  uint32_t key_len = 64;
  int ret = _sudt_build_key(SUDT_KEY_FLAG_BALANCE, address, key, &key_len);
  if (ret != 0) {
    return ret;
  }

  uint8_t value[32] = {0};
  _gw_fast_memcpy((uint8_t *)&value, (uint8_t *)(&balance), sizeof(uint256_t));
  ret = ctx->sys_store(ctx, sudt_id, key, key_len, value);
  return ret;
}

int sudt_get_balance(gw_context_t *ctx, const uint32_t sudt_id,
                     gw_reg_addr_t addr, uint256_t *balance) {
  int ret = gw_verify_sudt_account(ctx, sudt_id);
  if (ret != 0) {
    return ret;
  }
  return _sudt_get_balance(ctx, sudt_id, addr, balance);
}

int _sudt_get_total_supply(gw_context_t *ctx, const uint32_t sudt_id,
                           uint256_t *total_supply) {
  uint8_t value[32] = {0};
  int ret = ctx->sys_load(ctx, sudt_id, SUDT_TOTAL_SUPPLY_KEY, 32, value);
  if (ret != 0) {
    return ret;
  }
  _gw_fast_memcpy((uint8_t *)total_supply, (uint8_t *)(&value), 32);
  return 0;
}

int sudt_get_total_supply(gw_context_t *ctx, const uint32_t sudt_id,
                          uint256_t *total_supply) {
  int ret = gw_verify_sudt_account(ctx, sudt_id);
  if (ret != 0) {
    return ret;
  }
  return _sudt_get_total_supply(ctx, sudt_id, total_supply);
}

int _sudt_transfer(gw_context_t *ctx, const uint32_t sudt_id,
                   gw_reg_addr_t from_addr, gw_reg_addr_t to_addr,
                   const uint256_t amount, uint8_t service_flag) {
  int ret;
  ret = gw_verify_sudt_account(ctx, sudt_id);
  if (ret != 0) {
    printf("transfer: invalid sudt_id");
    return ret;
  }

  /* check from account */
  uint256_t from_balance = {0};
  ret = _sudt_get_balance(ctx, sudt_id, from_addr, &from_balance);
  if (ret != 0) {
    printf("transfer: can't get sender's balance");
    return ret;
  }
  if (gw_uint256_cmp(from_balance, amount) == GW_UINT256_SMALLER) {
    printf("transfer: insufficient balance");
    return GW_SUDT_ERROR_INSUFFICIENT_BALANCE;
  }

  if (_gw_cmp_addr(from_addr, to_addr) == 0) {
    printf("transfer: [warning] transfer to self");
  }

  uint256_t new_from_balance = {0};
  gw_uint256_underflow_sub(from_balance, amount, &new_from_balance);

  /* update sender balance */
  ret = _sudt_set_balance(ctx, sudt_id, from_addr, new_from_balance);
  if (ret != 0) {
    printf("transfer: update sender's balance failed");
    return ret;
  }

  /* check to account */
  uint256_t to_balance = {0};
  ret = _sudt_get_balance(ctx, sudt_id, to_addr, &to_balance);
  if (ret != 0) {
    printf("transfer: can't get receiver's balance");
    return ret;
  }

  uint256_t new_to_balance = {0};
  int overflow = gw_uint256_overflow_add(to_balance, amount, &new_to_balance);
  if (overflow) {
    printf("transfer: balance overflow");
    return GW_SUDT_ERROR_AMOUNT_OVERFLOW;
  }

  /* update receiver balance */
  ret = _sudt_set_balance(ctx, sudt_id, to_addr, new_to_balance);
  if (ret != 0) {
    printf("transfer: update receiver's balance failed");
    return ret;
  }

  /* emit log */
  ret = _sudt_emit_log(ctx, sudt_id, from_addr, to_addr, amount, service_flag);
  if (ret != 0) {
    printf("transfer: emit log failed");
  }
  return ret;
}

int sudt_transfer(gw_context_t *ctx, const uint32_t sudt_id,
                  gw_reg_addr_t from_addr, gw_reg_addr_t to_addr,
                  const uint256_t amount) {
  return _sudt_transfer(ctx, sudt_id, from_addr, to_addr, amount,
                        GW_LOG_SUDT_TRANSFER);
}

/* Pay fee */
int sudt_pay_fee(gw_context_t *ctx, const uint32_t sudt_id,
                 gw_reg_addr_t from_addr, const uint256_t amount) {
  /* transfer SUDT */
  int ret =
      _sudt_transfer(ctx, sudt_id, from_addr, ctx->block_info.block_producer,
                     amount, GW_LOG_SUDT_PAY_FEE);
  if (ret != 0) {
    printf("pay fee transfer failed");
    return ret;
  }

  /* call syscall, we use this action to emit event to runtime, this function
  do
   * not actually pay the fee */
  ret = ctx->sys_pay_fee(ctx, from_addr, sudt_id, amount);
  if (ret != 0) {
    printf("sys pay fee failed");
  }
  return ret;
}
