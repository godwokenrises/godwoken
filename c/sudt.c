/*
 * SUDT compatible layer2 contract
 * This contract is designed as the SUDT equivalent contract on layer2.
 *
 * Due to the fact this contract is used to receive layer1 assets,
 * this contract should supports all kind of SUDT.
 *
 * We use the type_script_hash of SUDT cells as token_id to destinguish
 * different tokens, which described in the RFC:
 * https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0025-simple-udt/0025-simple-udt.md#sudt-cell
 *
 * Basic APIs to supports transfer token:
 *
 * * query(token_id, to) -> value
 * * transfer(token_id, to, value)
 *
 * # Mint & Burn
 *
 * To join a Rollup, users deposite SUDT assets on layer1;
 * then Rollup aggregators take the layer1 assets and mint new SUDT coins on
 * layer2 according to the deposited assets.
 *
 * To leave a Rollup, the Rollup aggregators burn SUDT coins on layer2;
 * then send the layer2 SUDT assets to users.
 *
 * The aggregators operate Mint & Burn by directly modify the state tree.
 */

#define __SHARED_LIBRARY__ 1

#include "ckb_syscalls.h"
#include "godwoken.h"
#include "common.h"
#include "gw_def.h"
#include "stdio.h"

#define ERROR_INVALID_DATA 10
#define ERROR_UNKNOWN_MSG 11
#define ERROR_INSUFFICIENT_BALANCE 12
#define ERROR_AMOUNT_OVERFLOW 13

typedef unsigned __int128 uint128_t;

/* MSG_TYPE */
#define MSG_QUERY 0
#define MSG_TRANSFER 1

int query(gw_context_t *ctx, const uint8_t token_id[32],
          const uint32_t account_id, uint128_t *balance);
int transfer(gw_context_t *ctx, const uint8_t token_id[32],
             const uint32_t to_id, uint128_t amount);

/* do nothing on construct */
__attribute__((visibility("default"))) int gw_construct(gw_context_t *ctx) {
  return 0;
}

/* handle messages */
__attribute__((visibility("default"))) int gw_handle_message(
    gw_context_t *ctx) {
  /* parse SUDT args */
  mol_seg_t args_seg;
  args_seg.ptr = ctx->call_context.args;
  args_seg.size = ctx->call_context.args_len;
  if (MolReader_SUDTArgs_verify(&args_seg, false) != MOL_OK) {
    return ERROR_INVALID_DATA;
  }
  mol_union_t msg = MolReader_SUDTArgs_unpack(&args_seg);
  if (msg.item_id == MSG_QUERY) {
    /* Query */
    mol_seg_t token_id_seg = MolReader_SUDTQuery_get_token_id(&msg.seg);
    mol_seg_t account_id_seg = MolReader_SUDTQuery_get_account_id(&msg.seg);
    uint128_t value;
    int ret =
        query(ctx, token_id_seg.ptr, *(uint32_t *)account_id_seg.ptr, &value);
    if (ret != 0) {
      return ret;
    }
    ret = ctx->sys_set_program_return_data(ctx, (uint8_t *)&value, sizeof(uint128_t));
    if (ret != 0) {
      return ret;
    }
  } else if (msg.item_id == MSG_TRANSFER) {
    /* Transfer */
    mol_seg_t token_id_seg = MolReader_SUDTTransfer_get_to(&msg.seg);
    mol_seg_t to_seg = MolReader_SUDTTransfer_get_to(&msg.seg);
    mol_seg_t value_seg = MolReader_SUDTTransfer_get_value(&msg.seg);
    int ret = transfer(ctx, token_id_seg.ptr, *(uint32_t *)to_seg.ptr,
                       *(uint128_t *)value_seg.ptr);
    if (ret != 0) {
      return ret;
    }
  } else {
    return ERROR_UNKNOWN_MSG;
  }
  return 0;
}

void generate_key(gw_context_t *ctx, const uint8_t token_id[32],
                  const uint32_t account_id, uint8_t key[32]) {
  uint8_t buf[36];
  memcpy(buf, token_id, 32);
  memcpy(buf + 32, (uint8_t *)&account_id, sizeof(uint32_t));
  blake2b_hash(key, buf, 36);
  return;
}

int _get_balance(gw_context_t *ctx, const uint8_t raw_key[32],
                 uint128_t *balance) {
  uint8_t value[32];
  int ret = ctx->sys_load(ctx, raw_key, value);
  if (ret != 0) {
    return ret;
  }
  *balance = *(uint128_t *)value;
  return 0;
}

int query(gw_context_t *ctx, const uint8_t token_id[32],
          const uint32_t account_id, uint128_t *balance) {
  uint8_t key[32];
  generate_key(ctx, token_id, account_id, key);
  return _get_balance(ctx, key, balance);
}

int transfer(gw_context_t *ctx, const uint8_t token_id[32],
             const uint32_t to_id, uint128_t amount) {
  /* check from account */
  uint8_t from_key[32];
  generate_key(ctx, token_id, ctx->call_context.from_id, from_key);
  uint128_t from_balance;
  int ret = _get_balance(ctx, from_key, &from_balance);
  if (ret != 0) {
    return ret;
  }
  if (from_balance < amount) {
    return ERROR_INSUFFICIENT_BALANCE;
  }
  uint128_t new_from_balance = from_balance - amount;

  /* check to account */
  uint8_t to_key[32];
  generate_key(ctx, token_id, to_id, to_key);
  uint128_t to_balance;
  ret = _get_balance(ctx, to_key, &to_balance);
  if (ret != 0) {
    return ret;
  }
  uint128_t new_to_balance = to_balance + amount;
  if (new_to_balance < to_balance) {
    return ERROR_AMOUNT_OVERFLOW;
  }

  /* update balance */
  uint8_t from_value[32];
  *(uint128_t *)from_value = new_from_balance;
  ret = ctx->sys_store(ctx, from_key, from_value);
  if (ret != 0) {
    return ret;
  }

  uint8_t to_value[32];
  *(uint128_t *)to_value = new_to_balance;
  return ctx->sys_store(ctx, to_key, to_value);
}
