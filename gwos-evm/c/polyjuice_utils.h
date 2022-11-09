#ifndef POLYJUICE_UTILS_H
#define POLYJUICE_UTILS_H

#include <evmc/evmc.h>
#include <stddef.h>
#include <stdint.h>

#include "ckb_syscalls.h"
#include "polyjuice_errors.h"
#include "polyjuice_globals.h"

#ifdef POLYJUICE_DEBUG_LOG
/* 64 KB */
#define DEBUG_BUFFER_SIZE 65536
static char *g_debug_buffer;
void debug_print_data(const char *prefix, const uint8_t *data,
                      uint32_t data_len) {
  if (data_len > (DEBUG_BUFFER_SIZE - 1024) / 2 - 1) {  // leave 1KB to prefix
    ckb_debug("warning: length of data is too large");
    return;
  }

  int offset = 0;
  offset += sprintf(g_debug_buffer, "%s 0x", prefix);
  if (offset > 1024) {
    ckb_debug("warning: length of prefix is too large");
    return;
  }
  for (size_t i = 0; i < data_len; i++) {
    offset += sprintf(g_debug_buffer + offset, "%02x", data[i]);
  }
  g_debug_buffer[offset] = '\0';
  ckb_debug(g_debug_buffer);
}
void debug_print_int(const char *prefix, int64_t ret) {
  sprintf(g_debug_buffer, "%s => %ld", prefix, ret);
  ckb_debug(g_debug_buffer);
}
// avoid VM(InvalidEcall(80))
int printf(const char *format, ...) { return 0; }
#else
#undef ckb_debug
#define ckb_debug(s) \
  do {               \
  } while (0)
#define debug_print(s) \
  do {                 \
  } while (0)
#define debug_print_int(prefix, value) \
  do {                                 \
  } while (0)
#define debug_print_data(prefix, data, data_len) \
  do {                                           \
  } while (0)
int printf(const char *format, ...) { return 0; }
#endif /* POLYJUICE_DEBUG_LOG */

#define memset(dest, c, n) _smt_fast_memset(dest, c, n)

/* https://stackoverflow.com/a/1545079 */
#pragma push_macro("errno")
#undef errno
bool is_errno_ok(mol_seg_res_t *script_res) {
  return script_res->errno == MOL_OK;
}
#pragma pop_macro("errno")

gw_reg_addr_t new_reg_addr(const uint8_t eth_addr[ETH_ADDRESS_LEN]) {
  gw_reg_addr_t addr = {0};
  addr.reg_id = GW_DEFAULT_ETH_REGISTRY_ACCOUNT_ID;
  addr.addr_len = ETH_ADDRESS_LEN;
  memcpy(addr.addr, eth_addr, ETH_ADDRESS_LEN);
  return addr;
}

int build_script(const uint8_t code_hash[32], const uint8_t hash_type,
                 const uint8_t *args, const uint32_t args_len,
                 mol_seg_t *script_seg) {
  /* 1. Build Script by receipt.return_data */
  mol_seg_t args_seg;
  args_seg.size = 4 + args_len;
  args_seg.ptr = (uint8_t *)malloc(args_seg.size);
  if (args_seg.ptr == NULL) {
    return FATAL_POLYJUICE;
  }
  memcpy(args_seg.ptr, (uint8_t *)(&args_len), 4);
  memcpy(args_seg.ptr + 4, args, args_len);
  debug_print_int("script.hash_type", hash_type);

  mol_builder_t script_builder;
  MolBuilder_Script_init(&script_builder);
  MolBuilder_Script_set_code_hash(&script_builder, code_hash, 32);
  MolBuilder_Script_set_hash_type(&script_builder, hash_type);
  MolBuilder_Script_set_args(&script_builder, args_seg.ptr, args_seg.size);
  mol_seg_res_t script_res = MolBuilder_Script_build(script_builder);
  free(args_seg.ptr);

  if (!is_errno_ok(&script_res)) {
    ckb_debug("molecule build script failed");
    return FATAL_POLYJUICE;
  }

  *script_seg = script_res.seg;
  if (MolReader_Script_verify(script_seg, false) != MOL_OK) {
    ckb_debug("built an invalid script");
    return FATAL_POLYJUICE;
  }
  return 0;
}

/**
 * @param script_hash should have been initialed as zero_hash = {0}
 *
 * TODO: shall we cache the mapping data in Polyjuice memory?
 */
int load_script_hash_by_eth_address(gw_context_t *ctx,
                                    const uint8_t eth_address[ETH_ADDRESS_LEN],
                                    uint8_t script_hash[GW_VALUE_BYTES]) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  gw_reg_addr_t addr = new_reg_addr(eth_address);

  int ret = ctx->sys_get_script_hash_by_registry_address(ctx, &addr, script_hash);
  if (ret != 0) {
    return ret;
  }
  if (_is_zero_hash(script_hash)) {
    return GW_ERROR_NOT_FOUND;
  }
  return 0;
  ckb_debug("load_script_hash_by_eth_address success");
}

int load_eth_address_by_script_hash(gw_context_t *ctx,
                                    uint8_t script_hash[GW_KEY_BYTES],
                                    uint8_t eth_address[ETH_ADDRESS_LEN]) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  /* build addr */
  gw_reg_addr_t addr = new_reg_addr(eth_address);

  int ret = ctx->sys_get_registry_address_by_script_hash(ctx, script_hash, GW_DEFAULT_ETH_REGISTRY_ACCOUNT_ID, &addr);
  if (ret != 0) {
    return ret;
  }
  if (addr.addr_len == 0) {
    return GW_ERROR_NOT_FOUND;
  }

  _gw_fast_memcpy(eth_address, addr.addr, ETH_ADDRESS_LEN);
  return 0;
}

/**
 * @brief
 * TODO: test this function
 * @param ctx
 * @param address
 * @param account_id
 * @return int
 */
int load_account_id_by_eth_address(gw_context_t *ctx, const uint8_t address[20],
                                   uint32_t *account_id) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  uint8_t script_hash[32] = {0};
  int ret = load_script_hash_by_eth_address(ctx, address, script_hash);
  if (ret != 0) {
    debug_print_data("[load_account_id_by_eth_address] load_script_hash failed",
                     address, ETH_ADDRESS_LEN);
    return ret;
  }
  return ctx->sys_get_account_id_by_script_hash(ctx, script_hash, account_id);
}

void rlp_encode_sender_and_nonce(const evmc_address *sender, uint32_t nonce,
                                 uint8_t *data, uint32_t *data_len) {
  static const uint8_t RLP_ITEM_OFFSET = 0x80;
  static const uint8_t RLP_LIST_OFFSET = 0xc0;

  uint8_t *nonce_le = (uint8_t *)(&nonce);
  uint8_t nonce_be[4] = {0};
  nonce_be[0] = nonce_le[3];
  nonce_be[1] = nonce_le[2];
  nonce_be[2] = nonce_le[1];
  nonce_be[3] = nonce_le[0];
  uint32_t nonce_bytes_len = 0;
  for (size_t i = 0; i < 4; i++) {
    if (nonce_be[i] != 0) {
      nonce_bytes_len = 4 - i;
      break;
    }
  }

  /* == RLP encode == */
  /* sender header */
  data[1] = 20 + RLP_ITEM_OFFSET;
  /* sender content */
  memcpy(data + 2, sender->bytes, 20);
  if (nonce_bytes_len == 1 && nonce_be[3] < RLP_ITEM_OFFSET) {
    data[2 + 20] = nonce_be[3];
    *data_len = 2 + 20 + 1;
  } else {
    /* nonce header */
    data[2 + 20] = nonce_bytes_len + RLP_ITEM_OFFSET;
    /* nonce content */
    memcpy(data + 2 + 20 + 1, nonce_be + (4 - nonce_bytes_len),
           nonce_bytes_len);
    *data_len = 2 + 20 + 1 + nonce_bytes_len;
  }
  /* list header */
  data[0] = *data_len - 1 + RLP_LIST_OFFSET;
}

/* Parse uint32_t/uint64_t/uint128_t/uint256_t from big endian byte32 data */
int parse_integer(const uint8_t data_be[32], uint8_t *value,
                  size_t value_size) {
  if (value_size > 32) {
    return -1;
  }
  /* Check leading zeros */
  for (size_t i = 0; i < (32 - value_size); i++) {
    if (data_be[i] != 0) {
      return -1;
    }
  }

  for (size_t i = 0; i < value_size; i++) {
    value[i] = data_be[31 - i];
  }
  return 0;
}

int parse_u32(const uint8_t data_be[32], uint32_t *value) {
  return parse_integer(data_be, (uint8_t *)value, sizeof(uint32_t));
}
int parse_u64(const uint8_t data_be[32], uint64_t *value) {
  return parse_integer(data_be, (uint8_t *)value, sizeof(uint64_t));
}
int parse_u128(const uint8_t data_be[32], uint128_t *value) {
  return parse_integer(data_be, (uint8_t *)value, sizeof(uint128_t));
}
int parse_u256(const uint8_t data_be[32], uint256_t *value) {
  return parse_integer(data_be, (uint8_t *)value, sizeof(uint256_t));
}

uint128_t hi(uint128_t x) {
  return x >> 64;
}

uint128_t lo(uint128_t x) {
  return 0xFFFFFFFFFFFFFFFF & x;
}

/**
 * @brief calculate fee => gas_price * gas_used
 * 
 * Multiplication Algorithm
 * https://en.wikipedia.org/wiki/Multiplication_algorithm
 * 
 * @param gas_price  msg.gas_price
 * @param gas_used = msg.gas_limit - res.gas_left
 * @return uint256_t 
 */
uint256_t calculate_fee(uint128_t gas_price, uint64_t gas_used) {
  uint128_t price_high = hi(gas_price);
  uint128_t price_low = lo(gas_price);
  uint128_t fee_low = price_low * gas_used;
  uint128_t fee_high = hi(fee_low) + price_high * gas_used;

  uint256_t fee_u256 = {0};
  _gw_fast_memcpy((uint8_t *)(&fee_u256), (uint8_t*)(&fee_low), 8);
  _gw_fast_memcpy((uint8_t *)(&fee_u256) + 8, (uint8_t*)(&fee_high), 16);
  return fee_u256;
}

/* serialize uint64_t to big endian byte32 */
void put_u64(uint64_t value, uint8_t *output) {
  uint8_t *value_le = (uint8_t *)(&value);
  for (size_t i = 0; i < 8; i++) {
    *(output + 31 - i) = *(value_le + i);
  }
}

/* serialize uint128_t to big endian byte32 */
void put_u128(uint128_t value, uint8_t *output) {
  uint8_t *value_le = (uint8_t *)(&value);
  for (size_t i = 0; i < 16; i++) {
    *(output + 31 - i) = *(value_le + i);
  }
}

void put_u256(uint256_t value, uint8_t *output) {
  uint8_t *value_le = (uint8_t *)(&value);
  for (size_t i = 0; i < 32; i++) {
    *(output + 31 - i) = *(value_le + i);
  }
}

/* If it is a fatal error, terminate the whole process.
 * ====
 *   - gw_errors.h           GW_FATAIL_xxx               [50, 80)
 *   - polyjuice_globals.h   FATAL_POLYJUICE             -50
 *   - polyjuice_globals.h   FATAL_PRECOMPILED_CONTRACTS -51
 */
bool is_fatal_error(int error_code) {
  return (error_code >= 50 && error_code < 80) ||
         (error_code > -80 && error_code <= -50);
}

/* See evmc.h evmc_status_code */
bool is_evmc_error(int error_code) {
  return error_code >= 1 && error_code <= 16;
}

/**
 * @brief computes the 'intrinsic gas' for a message with the given data
 * 
 * @param msg evmc_message: transaction message
 * @param is_create bool: isContractCreation
 * @param min_gas the result of calculated intrinsic gas
 * @return int 
 */
int intrinsic_gas(const evmc_message *msg, const bool is_create,
                  uint64_t *min_gas) {
  // Set the starting gas for the raw transaction
  *min_gas = is_create ? MIN_CONTRACT_CREATION_TX_GAS : MIN_TX_GAS;

  // Bump the required gas by the size of transactional data
  if (msg->input_size > 0) {
    // Zero and non-zero bytes are priced differently
    uint64_t non_zero_bytes = 0;
    for (size_t i = 0; i < msg->input_size; i++) {
      if (*(msg->input_data + i) != 0)
        non_zero_bytes++;
    }

    // Make sure we don't exceed uint64 for all data combinations
    if ((UINT64_MAX - *min_gas) / DATA_NONE_ZERO_TX_GAS < non_zero_bytes) {
      return ERROR_INSUFFICIENT_GAS_LIMIT;
    }
    *min_gas += non_zero_bytes * DATA_NONE_ZERO_TX_GAS;

    uint64_t zero_bytes = msg->input_size - non_zero_bytes;
    if ((UINT64_MAX - *min_gas) / DATA_ZERO_TX_GAS < zero_bytes) {
      return ERROR_INSUFFICIENT_GAS_LIMIT;
    }
    *min_gas += zero_bytes * DATA_ZERO_TX_GAS;
  }

  return 0;
}

#endif  // POLYJUICE_UTILS_H
