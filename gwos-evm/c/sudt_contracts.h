
#ifndef SUDT_CONTRACTS_H_
#define SUDT_CONTRACTS_H_

#include "polyjuice_utils.h"

#define BALANCE_OF_ANY_SUDT_GAS 150
#define TOTAL_SUPPLY_OF_ANY_SUDT_GAS 150
#define TRANSFER_TO_ANY_SUDT_GAS 300

int balance_of_any_sudt_gas(const uint8_t* input_src, const size_t input_size,
                            uint64_t* gas) {
  *gas = BALANCE_OF_ANY_SUDT_GAS;
  return 0;
}

/*
  Query the balance of `account_id` of `sudt_id` token.

   input:
   ======
     input[ 0..32] => sudt_id (big endian)
     input[32..64] => address (eth_address)

   output:
   =======
     output[0..32] => amount
 */
int balance_of_any_sudt(gw_context_t* ctx,
                        const uint8_t* msg_sender,
                        const enum evmc_call_kind parent_kind,
                        bool is_static_call,
                        const uint8_t* input_src, const size_t input_size,
                        uint8_t** output, size_t* output_size) {
  int ret;
  if (input_size != (32 + 32)) {
    return ERROR_BALANCE_OF_ANY_SUDT;
  }

  uint32_t sudt_id = 0;
  ret = parse_u32(input_src, &sudt_id);
  if (ret != 0) {
    return ERROR_BALANCE_OF_ANY_SUDT;
  }

  // Default return zero balance
  *output = (uint8_t*)malloc(32);
  if (*output == NULL) {
    ckb_debug("[balance_of_any_sudt] malloc failed");
    return FATAL_PRECOMPILED_CONTRACTS;
  }
  *output_size = 32;
  memset(*output, 0, 32);

  for (int i = 0; i < 12; i++) {
    if (input_src[32 + i] != 0) {
      ckb_debug("[balance_of_any_sudt] invalid ethereum address");
      return ERROR_BALANCE_OF_ANY_SUDT;
    }
  }
  evmc_address address = *((evmc_address*)(input_src + 32 + 12));

  gw_reg_addr_t addr = new_reg_addr(address.bytes);

  uint256_t balance;
  ret = sudt_get_balance(ctx, sudt_id, addr, &balance);
  if (ret == GW_ERROR_NOT_FOUND) {
    debug_print_int("[balance_of_any_sudt] sudt account not found", sudt_id);
    return 0;
  } else if (ret != 0) {
    debug_print_int("[balance_of_any_sudt] sudt_get_balance failed", ret);
    if (is_fatal_error(ret)) {
      return FATAL_PRECOMPILED_CONTRACTS;
    } else {
      return ERROR_BALANCE_OF_ANY_SUDT;
    }
  }
  put_u256(balance, *output);
  return 0;
}

int total_supply_of_any_sudt_gas(const uint8_t* input_src,
                                 const size_t input_size, uint64_t* gas) {
  *gas = TOTAL_SUPPLY_OF_ANY_SUDT_GAS;
  return 0;
}

/*
  Query the total supply of `sudt_id` token.

   input:
   ======
     input[ 0..32] => sudt_id (big endian)

   output:
   =======
     output[0..32] => amount
 */
int total_supply_of_any_sudt(gw_context_t* ctx,
                             const uint8_t* msg_sender,
                             const enum evmc_call_kind parent_kind,
                             bool is_static_call,
                             const uint8_t* input_src, const size_t input_size,
                             uint8_t** output, size_t* output_size) {
  int ret;
  if (input_size != 32) {
    return ERROR_TOTAL_SUPPLY_OF_ANY_SUDT;
  }

  uint32_t sudt_id = 0;
  ret = parse_u32(input_src, &sudt_id);
  if (ret != 0) {
    return ERROR_TOTAL_SUPPLY_OF_ANY_SUDT;
  }

  // Default return zero total supply
  *output = (uint8_t*)malloc(32);
  if (*output == NULL) {
    ckb_debug("malloc failed");
    return FATAL_PRECOMPILED_CONTRACTS;
  }
  *output_size = 32;
  memset(*output, 0, 32);

  uint256_t total_supply_le = {0};
  ret = sudt_get_total_supply(ctx, sudt_id, &total_supply_le);
  if (ret == GW_ERROR_NOT_FOUND) {
    debug_print_int("sudt account not found", sudt_id);
    return 0;
  } else if (ret != 0) {
    debug_print_int("sudt_get_total_supply failed", ret);
    if (is_fatal_error(ret)) {
      return FATAL_PRECOMPILED_CONTRACTS;
    } else {
      return ERROR_TOTAL_SUPPLY_OF_ANY_SUDT;
    }
  }

  uint8_t* total_supply_le_bytes = (uint8_t*)&total_supply_le;
  for (size_t i = 0; i < 32; i++) {
    (*output)[31 - i] = total_supply_le_bytes[i];
  }
  return 0;
}

int transfer_to_any_sudt_gas(const uint8_t* input_src, const size_t input_size,
                             uint64_t* gas) {
  *gas = TRANSFER_TO_ANY_SUDT_GAS;
  return 0;
}

/*
  Transfer `sudt_id` token from `from_id` to `to_id` with `amount` balance.

  NOTE: This pre-compiled contract need caller to check permission of `from_id`,
  currently only `solidity/erc20/SudtERC20Proxy_UserDefinedDecimals.sol` is
  allowed to call this contract.

   input:
   ======
     input[ 0..32 ] => sudt_id (big endian)
     input[32..64 ] => from_addr (eth address)
     input[64..96 ] => to_addr (eth address)
     input[96..128] => amount (big endian)

   output: []
 */
int transfer_to_any_sudt(gw_context_t* ctx,
                         const uint8_t* msg_sender,
                         const enum evmc_call_kind parent_kind,
                         bool is_static_call,
                         const uint8_t* input_src, const size_t input_size,
                         uint8_t** output, size_t* output_size) {
  /* check msg_sender is in allow list */
  int ret = ctx->sys_check_sudt_addr_permission(ctx, msg_sender);
  if (ret != 0) {
    ckb_debug("Disallowed sUDT proxy contract");
    return ERROR_TRANSFER_TO_ANY_SUDT;
  }

  if (is_static_call) {
    ckb_debug("static call to transfer to any sudt is forbidden");
    return ERROR_TRANSFER_TO_ANY_SUDT;
  }
  if (parent_kind == EVMC_CALLCODE || parent_kind == EVMC_DELEGATECALL) {
    ckb_debug("delegatecall/callcode to transfer to any sudt is forbidden");
    return ERROR_TRANSFER_TO_ANY_SUDT;
  }
  if (input_size != (32 + 32 + 32 + 32)) {
    return ERROR_TRANSFER_TO_ANY_SUDT;
  }

  uint32_t sudt_id = 0;
  uint256_t amount = {0};
  ret = parse_u32(input_src, &sudt_id);
  if (ret != 0) {
    return ERROR_TRANSFER_TO_ANY_SUDT;
  }
  ret = parse_u256(input_src + 96, &amount);
  if (ret != 0) {
    return ERROR_TRANSFER_TO_ANY_SUDT;
  }

  gw_reg_addr_t from_addr = new_reg_addr(input_src + 32 + 12);
  gw_reg_addr_t to_addr = new_reg_addr(input_src + 64 + 12);

  ret = sudt_transfer(ctx, sudt_id, from_addr, to_addr, amount);
  if (ret != 0) {
    debug_print_int("[transfer_to_any_sudt] transfer failed", ret);
    if (is_fatal_error(ret)) {
      return FATAL_PRECOMPILED_CONTRACTS;
    } else {
      return ERROR_TRANSFER_TO_ANY_SUDT;
    }
  }

  *output = NULL;
  *output_size = 0;
  return 0;
}

#endif /* #define SUDT_CONTRACTS_H_ */
