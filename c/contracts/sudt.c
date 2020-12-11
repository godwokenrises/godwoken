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

#include "ckb_syscalls.h"
#include "gw_syscalls.h"
#include "stdio.h"
#include "sudt_utils.h"

/* MSG_TYPE */
#define MSG_QUERY 0
#define MSG_TRANSFER 1
#define MSG_PREPAREWITHDRAWAL 2

int main() {
  /* initialize context */
  gw_context_t ctx = {0};
  int ret = gw_context_init(&ctx);
  if (ret != 0) {
    return ret;
  };

  /* parse SUDT args */
  mol_seg_t args_seg;
  args_seg.ptr = ctx.transaction_context.args;
  args_seg.size = ctx.transaction_context.args_len;
  if (MolReader_SUDTArgs_verify(&args_seg, false) != MOL_OK) {
    return ERROR_INVALID_DATA;
  }
  mol_union_t msg = MolReader_SUDTArgs_unpack(&args_seg);
  uint32_t sudt_id = ctx.transaction_context.to_id;

  /* Handle messages */
  if (msg.item_id == MSG_QUERY) {
    /* Query */
    mol_seg_t account_id_seg = MolReader_SUDTQuery_get_account_id(&msg.seg);
    uint32_t account_id = *(uint32_t *)account_id_seg.ptr;
    uint128_t balance = 0;
    int ret = sudt_get_balance(&ctx, sudt_id, account_id, &balance);
    if (ret != 0) {
      return ret;
    }
    ret = ctx.sys_set_program_return_data(&ctx, (uint8_t *)&balance,
                                          sizeof(uint128_t));
    if (ret != 0) {
      return ret;
    }
  } else if (msg.item_id == MSG_TRANSFER) {
    /* Transfer */
    mol_seg_t to_seg = MolReader_SUDTTransfer_get_to(&msg.seg);
    mol_seg_t amount_seg = MolReader_SUDTTransfer_get_amount(&msg.seg);
    mol_seg_t fee_seg = MolReader_SUDTTransfer_get_fee(&msg.seg);
    uint32_t from_id = ctx.transaction_context.from_id;
    uint32_t to_id = *(uint32_t *)to_seg.ptr;
    uint128_t amount = *(uint128_t *)amount_seg.ptr;
    uint128_t fee = *(uint128_t *)fee_seg.ptr;
    /* pay fee */
    int ret = sudt_transfer(&ctx, sudt_id, from_id,
                            ctx.block_info.aggregator_id, fee);
    if (ret != 0) {
      return ret;
    }
    /* transfer */
    ret = sudt_transfer(&ctx, sudt_id, from_id, to_id, amount);
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
    int ret =
        sudt_prepare_withdrawal(&ctx, withdrawal_lock_hash_seg.ptr, amount);
    if (ret != 0) {
      return ret;
    }
  } else {
    return ERROR_UNKNOWN_MSG;
  }
  return 0;
}
