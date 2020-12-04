/*
 * SUDT compatible layer2 contract
 * This contract is designed as the SUDT equivalent contract on layer2.
 *
 * One layer2 SUDT contract is mapping to one layer1 SUDT contract
 *
 * We use the sudt_script_hash of SUDT cells in layer2 script args to
 * destinguish different SUDT tokens, which described in the RFC:
 * https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0025-simple-udt/0025-simple-udt.md#sudt-cell
 *
 * Basic APIs to supports transfer token:
 *
 * * sudt_script_hash() -> H256
 * * query(account_id) -> balance
 * * transfer(to, amount, fee)
 * * prepare_withdrawal(withdraw_lock_hash, amount, fee)
 *
 * # Mint & Burn
 *
 * To join a Rollup, users deposite SUDT assets on layer1;
 * then Rollup aggregators take the layer1 assets and mint new SUDT coins on
 * layer2 according to the deposited assets.
 * (Aggregator find a corresponded layer2 SUDT contract by searching
 * sudt_script_hash, or create one if the SUDT hasn't been deposited before)
 *
 * To leave a Rollup, users firstly call prepare_withdrawal on SUDT contract,
 * then after a confirmation time, the Rollup aggregators burn SUDT coins from
 * layer2 and send the layer1 SUDT assets to users.
 *
 * The aggregators operate Mint & Burn by directly modify the state tree.
 */

#define __SHARED_LIBRARY__ 1

#include "ckb_syscalls.h"
#include "common.h"
#include "godwoken.h"
#include "gw_def.h"
#include "stdio.h"

/* errors */
#define ERROR_INVALID_DATA 10
#define ERROR_UNKNOWN_MSG 11
#define ERROR_INSUFFICIENT_BALANCE 12
#define ERROR_AMOUNT_OVERFLOW 13

typedef unsigned __int128 uint128_t;

/* MSG_TYPE */
#define MSG_SCRIPTHASH 0
#define MSG_QUERY 1
#define MSG_TRANSFER 2
#define MSG_PREPAREWITHDRAWAL 3

/* Prepare withdrawal fields */
#define WITHDRAWAL_LOCK_HASH 1
#define WITHDRAWAL_AMOUNT 2
#define WITHDRAWAL_BLOCK_NUMBER 3

/* BLAKE2b of "SUDT_SCRIPT_HASH" */
const static uint8_t SUDT_SCRIPT_HASH[32] = {0};

int sudt_script_hash(gw_context_t *ctx, uint8_t sudt_script_hash[32]);
int balance(gw_context_t *ctx, const uint8_t account_key[32],
            uint128_t *balance);
int transfer(gw_context_t *ctx, const uint8_t to_account_key[32],
             uint128_t amount);
int prepare_withdrawal(gw_context_t *ctx,
                       const uint8_t withdrawal_lock_hash[32],
                       uint128_t amount);
void _id_to_key(const uint32_t account_id, uint8_t account_key[32]);

/* do nothing on construct */
__attribute__((visibility("default"))) int gw_construct(gw_context_t *ctx) {
  if (ctx->args_len != 32) {
    return ERROR_INVALID_DATA;
  }
  return ctx->sys_store(ctx, SUDT_SCRIPT_HASH, ctx->args);
}

/* handle messages */
__attribute__((visibility("default"))) int
gw_handle_message(gw_context_t *ctx) {
  /* parse SUDT args */
  mol_seg_t args_seg;
  args_seg.ptr = ctx->call_context.args;
  args_seg.size = ctx->call_context.args_len;
  if (MolReader_SUDTArgs_verify(&args_seg, false) != MOL_OK) {
    return ERROR_INVALID_DATA;
  }
  mol_union_t msg = MolReader_SUDTArgs_unpack(&args_seg);

  /* Handle messages */
  if (msg.item_id == MSG_SCRIPTHASH) {
    uint8_t sudt_script_hash[32] = {0};
    int ret = ctx->sys_load(ctx, SUDT_SCRIPT_HASH, sudt_script_hash);
    if (ret != 0) {
      return ret;
    }
    ret = ctx->sys_set_program_return_data(ctx, sudt_script_hash, 32);
    if (ret != 0) {
      return ret;
    }
  } else if (msg.item_id == MSG_QUERY) {
    /* Query */
    mol_seg_t account_id_seg = MolReader_SUDTQuery_get_account_id(&msg.seg);
    uint8_t key[32] = {0};
    _id_to_key(*(uint32_t *)account_id_seg.ptr, key);
    uint128_t balance = 0;
    int ret = get_balance(ctx, key, &balance);
    if (ret != 0) {
      return ret;
    }
    ret = ctx->sys_set_program_return_data(ctx, (uint8_t *)&balance,
                                           sizeof(uint128_t));
    if (ret != 0) {
      return ret;
    }
  } else if (msg.item_id == MSG_TRANSFER) {
    /* Transfer */
    mol_seg_t to_seg = MolReader_SUDTTransfer_get_to(&msg.seg);
    mol_seg_t amount_seg = MolReader_SUDTTransfer_get_amount(&msg.seg);
    int ret =
        transfer(ctx, *(uint32_t *)to_seg.ptr, *(uint128_t *)amount_seg.ptr);
    if (ret != 0) {
      return ret;
    }
  } else if (msg.item_id == MSG_PREPAREWITHDRAWAL) {
    /* Prepare withdrawal */
    mol_seg_t withdrawal_lock_hash_seg =
        MolReader_SUDTPrepareWithdrawal_get_withdrawal_lock_hash(&msg.seg);
    mol_seg_t amount_seg = MolReader_SUDTPrepareWithdrawal_get_amount(&msg.seg);
    uint128_t amount = *(uint128_t *)amount_seg.ptr;
    if (amount == 0) {
      return ERROR_INVALID_DATA;
    }
    int ret = prepare_withdrawal(withdrawal_lock_hash_seg.ptr, amount);
    if (ret != 0) {
      return ret;
    }
  } else {
    return ERROR_UNKNOWN_MSG;
  }
  return 0;
}

void _id_to_key(const uint32_t account_id, uint8_t key[32]) {
  memcpy(key, account_id, 4);
}

int get_balance(gw_context_t *ctx, uint8_t key[32], uint128_t *balance) {
  uint8_t value[32] = {0};
  int ret = ctx->sys_load(ctx, key, value);
  if (ret != 0) {
    return ret;
  }
  *balance = *(uint128_t *)value;
  return 0;
}

int set_balance(gw_context_t *ctx, uint8_t key[32], uint128_t balance) {
  uint8_t value[32] = {0};
  *(uint128_t *)value = balance;
  int ret = ctx->sys_store(ctx, key, value);
  return ret;
}

int transfer(gw_context_t *ctx, const uint32_t to_id, uint128_t amount) {
  /* check from account */
  uint8_t from_key[32] = {0};
  _id_to_key(ctx->call_context.from_id, from_key);
  uint128_t from_balance;
  int ret = get_balance(ctx, from_key, &from_balance);
  if (ret != 0) {
    return ret;
  }
  if (from_balance < amount) {
    return ERROR_INSUFFICIENT_BALANCE;
  }
  uint128_t new_from_balance = from_balance - amount;

  /* check to account */
  uint8_t to_key[32] = {0};
  _id_to_key(to_id, to_key);
  uint128_t to_balance;
  ret = get_balance(ctx, to_key, &to_balance);
  if (ret != 0) {
    return ret;
  }
  uint128_t new_to_balance = to_balance + amount;
  if (new_to_balance < to_balance) {
    return ERROR_AMOUNT_OVERFLOW;
  }

  /* update balance */
  ret = set_balance(from_key, new_from_balance);
  if (ret != 0) {
    return ret;
  }
  return set_balance(to_key, new_to_balance);
}

int prepare_withdrawal(gw_context_t *ctx,
                       const uint8_t withdrawal_lock_hash[32],
                       uint128_t amount) {
  /* store prepare withdrawal (account_id, block_number, withdrawal_lock_hash,
   * amount) */
  return 0;
}
