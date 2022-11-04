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
 *
 * # Mint & Burn
 *
 * To join a Rollup, users deposit SUDT assets on layer1;
 * then Rollup aggregators take the layer1 assets and mint new SUDT coins on
 * layer2 according to the deposited assets.
 * (Aggregator find a corresponded layer2 SUDT contract by searching
 * sudt_script_hash, or create one if the SUDT hasn't been deposited before)
 *
 * To leave a Rollup, the Rollup aggregators burn SUDT coins from layer2 and
 * send the layer1 SUDT assets to users.
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
    return GW_FATAL_INVALID_DATA;
  }
  mol_union_t msg = MolReader_SUDTArgs_unpack(&args_seg);
  uint32_t sudt_id = ctx.transaction_context.to_id;

  /* Handle messages */
  if (msg.item_id == MSG_QUERY) {
    /* Query */
    mol_seg_t address_seg = MolReader_SUDTQuery_get_address(&msg.seg);
    mol_seg_t raw_address_seg = MolReader_Bytes_raw_bytes(&address_seg);
    gw_reg_addr_t addr;
    ret = _gw_parse_addr((uint8_t *)raw_address_seg.ptr, raw_address_seg.size,
                         &addr);
    if (ret != 0) {
      return ret;
    }
    uint256_t balance = {0};
    ret = sudt_get_balance(&ctx, sudt_id, addr, &balance);
    if (ret != 0) {
      return ret;
    }
    ret = ctx.sys_set_program_return_data(&ctx, (uint8_t *)&balance,
                                          sizeof(uint256_t));
    if (ret != 0) {
      return ret;
    }
  } else if (msg.item_id == MSG_TRANSFER) {
    /* Transfer */
    mol_seg_t to_seg = MolReader_SUDTTransfer_get_to_address(&msg.seg);
    mol_seg_t raw_to_seg = MolReader_Bytes_raw_bytes(&to_seg);

    mol_seg_t amount_seg = MolReader_SUDTTransfer_get_amount(&msg.seg);
    mol_seg_t fee_seg = MolReader_SUDTTransfer_get_fee(&msg.seg);
    mol_seg_t fee_amount_seg = MolReader_Fee_get_amount(&fee_seg);
    mol_seg_t fee_reg_seg = MolReader_Fee_get_registry_id(&fee_seg);

    uint256_t fee_amount = {0};
    _gw_fast_memcpy((uint8_t *)(&fee_amount), (uint8_t *)fee_amount_seg.ptr,
                    sizeof(uint128_t));

    uint32_t reg_id = 0;
    _gw_fast_memcpy((uint8_t *)(&reg_id), fee_reg_seg.ptr, sizeof(uint32_t));

    uint32_t from_id = ctx.transaction_context.from_id;
    uint8_t from_script_hash[32] = {0};
    ret =
        ctx.sys_get_script_hash_by_account_id(&ctx, from_id, from_script_hash);
    if (ret != 0) {
      return ret;
    }
    /* Address */
    gw_reg_addr_t from_addr;
    ret = ctx.sys_get_registry_address_by_script_hash(&ctx, from_script_hash,
                                                      reg_id, &from_addr);
    if (ret != 0) {
      return ret;
    }

    gw_reg_addr_t to_addr;
    ret = _gw_parse_addr(raw_to_seg.ptr, raw_to_seg.size, &to_addr);
    if (ret != 0) {
      return ret;
    }

    uint256_t amount = {0};
    _gw_fast_memcpy((uint8_t *)(&amount), (uint8_t *)amount_seg.ptr,
                    sizeof(uint256_t));

    /* pay fee */
    ret = sudt_pay_fee(&ctx, CKB_SUDT_ACCOUNT_ID, from_addr, fee_amount);
    if (ret != 0) {
      printf("pay fee failed");
      return ret;
    }
    /* transfer */
    ret = sudt_transfer(&ctx, sudt_id, from_addr, to_addr, amount);
    if (ret != 0) {
      printf("transfer token failed");
      return ret;
    }
  } else {
    return GW_FATAL_UNKNOWN_ARGS;
  }

  return gw_finalize(&ctx);
}
