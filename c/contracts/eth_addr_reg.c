/**
 * `ETH Address Registry` layer2 contract
 *
 * This contract introduces two-ways mappings between `eth_address` and
 * `gw_script_hash`.
 *
 *   - `eth_address` is the address of an Ethereum EOA (externally owned account
 *     ) or a Polyjuice contract account.
 *
 *   - Godwoken account script hash(a.k.a. `gw_script_hash`) is a key used for
 *     locating the account lock. Godwoken enforces one-to-one mapping between
 *     layer 2 lock script and accountID.
 */

#include "gw_eth_addr_reg.h"
#include "sudt_utils.h"

/* MSG_TYPE */
#define MSG_QUERY_GW_BY_ETH 0
#define MSG_QUERY_ETH_BY_GW 1
#define MSG_SET_MAPPING 2
#define MSG_BATCH_SET_MAPPING 3

int handle_fee(gw_context_t *ctx, uint32_t registry_id, uint256_t amount) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  /* payer's registry address */
  uint8_t payer_script_hash[32] = {0};
  int ret = ctx->sys_get_script_hash_by_account_id(
      ctx, ctx->transaction_context.from_id, payer_script_hash);
  if (ret != 0) {
    return ret;
  }
  gw_reg_addr_t payer_addr;
  ret = ctx->sys_get_registry_address_by_script_hash(ctx, payer_script_hash,
                                                     registry_id, &payer_addr);
  if (ret != 0) {
    return ret;
  }

  return sudt_pay_fee(ctx, CKB_SUDT_ACCOUNT_ID, payer_addr, amount);
}

int main() {
  ckb_debug("====== ETH Address Registry ======");

  /* initialize context */
  gw_context_t ctx = {0};
  int ret = gw_context_init(&ctx);
  if (ret != 0) {
    return ret;
  };

  /* verify and parse args */
  mol_seg_t args_seg;
  args_seg.ptr = ctx.transaction_context.args;
  args_seg.size = ctx.transaction_context.args_len;
  if (MolReader_ETHAddrRegArgs_verify(&args_seg, false) != MOL_OK) {
    return GW_FATAL_INVALID_DATA;
  }
  mol_union_t msg = MolReader_ETHAddrRegArgs_unpack(&args_seg);

  /* handle message */
  if (msg.item_id == MSG_QUERY_GW_BY_ETH) {
    mol_seg_t eth_address_seg = MolReader_EthToGw_get_eth_address(&msg.seg);
    uint8_t script_hash[GW_VALUE_BYTES] = {0};
    /* addr */
    gw_reg_addr_t addr;
    memcpy(addr.addr, eth_address_seg.ptr, 20);
    addr.addr_len = 20;
    addr.reg_id = ctx.transaction_context.to_id;
    /* get script hash */
    ret = ctx.sys_get_script_hash_by_registry_address(&ctx, &addr, script_hash);
    if (ret != 0) {
      return ret;
    }
    ret = ctx.sys_set_program_return_data(&ctx, script_hash, GW_VALUE_BYTES);
    if (ret != 0) {
      return ret;
    }
  } else if (msg.item_id == MSG_QUERY_ETH_BY_GW) {
    mol_seg_t script_hash_seg = MolReader_GwToEth_get_gw_script_hash(&msg.seg);
    gw_reg_addr_t addr;
    ret = ctx.sys_get_registry_address_by_script_hash(
        &ctx, script_hash_seg.ptr, ctx.transaction_context.to_id, &addr);
    if (ret != 0) {
      return ret;
    }
    if (addr.addr_len != GW_ETH_ADDRESS_LEN) {
      return GW_FATAL_INVALID_DATA;
    }
    ret = ctx.sys_set_program_return_data(&ctx, addr.addr, GW_ETH_ADDRESS_LEN);
    if (ret != 0) {
      return ret;
    }
  } else if (msg.item_id == MSG_SET_MAPPING) {
    mol_seg_t script_hash_seg =
        MolReader_SetMapping_get_gw_script_hash(&msg.seg);
    ret = gw_register_eth_address(&ctx, script_hash_seg.ptr);
    if (ret != 0) {
      return ret;
    }
    /* charge fee */
    mol_seg_t fee_seg = MolReader_SetMapping_get_fee(&msg.seg);
    mol_seg_t amount_seg = MolReader_Fee_get_amount(&fee_seg);
    mol_seg_t reg_id_seg = MolReader_Fee_get_registry_id(&fee_seg);

    uint32_t reg_id = 0;
    _gw_fast_memcpy((uint8_t *)(&reg_id), reg_id_seg.ptr, sizeof(uint32_t));

    uint256_t fee_amount = {0};
    _gw_fast_memcpy((uint8_t *)(&fee_amount), (uint8_t *)amount_seg.ptr,
                    sizeof(uint128_t));

    ret = handle_fee(&ctx, reg_id, fee_amount);
    if (ret != 0) {
      return ret;
    }

  } else if (msg.item_id == MSG_BATCH_SET_MAPPING) {
    mol_seg_t script_hashes_seg =
        MolReader_BatchSetMapping_get_gw_script_hashes(&msg.seg);
    uint32_t script_hashes_size =
        MolReader_Byte32Vec_length(&script_hashes_seg);

    for (uint32_t i = 0; i < script_hashes_size; i++) {
      mol_seg_res_t script_hash_res =
          MolReader_Byte32Vec_get(&script_hashes_seg, i);
      if (script_hash_res.errno != MOL_OK) {
        ckb_debug("invalid script hash");
        return GW_FATAL_INVALID_DATA;
      }
      ret = gw_register_eth_address(&ctx, script_hash_res.seg.ptr);
      if (ret != 0) {
        return ret;
      }
    }
    /* charge fee */
    mol_seg_t fee_seg = MolReader_BatchSetMapping_get_fee(&msg.seg);
    mol_seg_t amount_seg = MolReader_Fee_get_amount(&fee_seg);
    mol_seg_t reg_id_seg = MolReader_Fee_get_registry_id(&fee_seg);

    uint32_t reg_id = 0;
    _gw_fast_memcpy((uint8_t *)(&reg_id), reg_id_seg.ptr, sizeof(uint32_t));

    uint256_t fee_amount = {0};
    _gw_fast_memcpy((uint8_t *)(&fee_amount), (uint8_t *)amount_seg.ptr,
                    sizeof(uint128_t));

    ret = handle_fee(&ctx, reg_id, fee_amount);
    if (ret != 0) {
      return ret;
    }

  } else {
    return GW_FATAL_UNKNOWN_ARGS;
  }

  return gw_finalize(&ctx);
}
