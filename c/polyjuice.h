#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "ckb_syscalls.h"

#include <ethash/keccak.hpp>
#include <evmc/evmc.h>
#include <evmc/evmc.hpp>
#include <evmone/evmone.h>

#include "uint256.h"

/* https://stackoverflow.com/a/1545079 */
#pragma push_macro("errno")
#undef errno
#include "godwoken.h"
#include "gw_eth_addr_reg.h"
#include "gw_syscalls.h"
#include "sudt_utils.h"
#pragma pop_macro("errno")

#include "common.h"
#include "polyjuice_errors.h"
#include "polyjuice_utils.h"

#ifdef GW_GENERATOR
#include "generator/secp256k1_helper.h"
#else
#include "validator/secp256k1_helper.h"
#endif
#include "contracts.h"

#define is_create(kind) ((kind) == EVMC_CREATE || (kind) == EVMC_CREATE2)
#define is_special_call(kind) \
  ((kind) == EVMC_CALLCODE || (kind) == EVMC_DELEGATECALL)

/* Max data buffer size: 24KB */
#define MAX_DATA_SIZE 24576
/* Max evm_memory size 512KB */
#define MAX_EVM_MEMORY_SIZE 524288
#define POLYJUICE_SYSTEM_PREFIX 0xFF
#define POLYJUICE_CONTRACT_CODE 0x01
#define POLYJUICE_DESTRUCTED 0x02

void polyjuice_build_system_key(uint32_t id, uint8_t polyjuice_field_type,
                                uint8_t key[GW_KEY_BYTES]) {
  memset(key, 0, GW_KEY_BYTES);
  memcpy(key, (uint8_t*)(&id), sizeof(uint32_t));
  key[4] = POLYJUICE_SYSTEM_PREFIX;
  key[5] = polyjuice_field_type;
}

void polyjuice_build_contract_code_key(uint32_t id, uint8_t key[GW_KEY_BYTES]) {
  polyjuice_build_system_key(id, POLYJUICE_CONTRACT_CODE, key);
}
void polyjuice_build_destructed_key(uint32_t id, uint8_t key[GW_KEY_BYTES]) {
  polyjuice_build_system_key(id, POLYJUICE_DESTRUCTED, key);
}

/* assume `account_id` already exists */
int gw_increase_nonce(gw_context_t *ctx, uint32_t account_id, uint32_t *new_nonce) {
  uint32_t old_nonce;
  int ret = ctx->sys_get_account_nonce(ctx, account_id, &old_nonce);
  if (ret != 0) {
    return ret;
  }
  uint32_t next_nonce = old_nonce + 1;

  uint8_t nonce_key[GW_KEY_BYTES];
  uint8_t nonce_value[GW_VALUE_BYTES];
  memset(nonce_value, 0, GW_VALUE_BYTES);
  gw_build_account_field_key(account_id, GW_ACCOUNT_NONCE, nonce_key);
  memcpy(nonce_value, (uint8_t *)(&next_nonce), 4);
  ret = ctx->_internal_store_raw(ctx, nonce_key, nonce_value);
  if (ret != 0) {
    return ret;
  }
  if (new_nonce != NULL) {
    *new_nonce = next_nonce;
  }
  return 0;
}

int handle_message(gw_context_t* ctx,
                   uint32_t parent_from_id,
                   uint32_t parent_to_id,
                   evmc_address *parent_destination,
                   const evmc_message* msg,
                   struct evmc_result* res);
typedef int (*stream_data_loader_fn)(gw_context_t* ctx, long data_id,
                                     uint32_t* len, uint32_t offset,
                                     uint8_t* data);

struct evmc_host_context {
  gw_context_t* gw_ctx;
  const uint8_t* code_data;
  const size_t code_size;
  // parent level call kind
  enum evmc_call_kind kind;
  uint32_t from_id;
  uint32_t to_id;
  // parent level sender
  evmc_address sender;
  // parent level destination
  evmc_address destination;
  int error_code;
};

int load_account_script(gw_context_t* gw_ctx, uint32_t account_id,
                        uint8_t* buffer, uint32_t buffer_size,
                        mol_seg_t* script_seg) {
  debug_print_int("load_account_script, account_id:", account_id);
  int ret;
  uint64_t len = buffer_size;
  ret = gw_ctx->sys_get_account_script(gw_ctx, account_id, &len, 0, buffer);
  if (ret != 0) {
    ckb_debug("load account script failed");
    return ret;
  }
  script_seg->ptr = buffer;
  script_seg->size = len;
  if (MolReader_Script_verify(script_seg, false) != MOL_OK) {
    ckb_debug("load account script: invalid script");
    return FATAL_POLYJUICE;
  }
  return 0;
}

// TODO: change gas_limit, gas_price, value to u256
/**
   Message = [
     header     : [u8; 8]            0xff, 0xff, 0xff, "POLY", call_kind
     gas_limit  : u64                (little endian)
     gas_price  : u128               (little endian)
     value      : u128               (little endian)
     input_size : u32                (little endian)
     input_data : [u8; input_size]
     to_address : [u8; 20]	     optional, must be an EOA 
   ]
 */
int parse_args(struct evmc_message* msg, gw_context_t* ctx) {
  gw_transaction_context_t *tx_ctx = &ctx->transaction_context;
  debug_print_int("args_len", tx_ctx->args_len);
  if (tx_ctx->args_len < (8 + 8 + 16 + 16 + 4)) {
    ckb_debug("invalid polyjuice arguments data");
    return -1;
  }
  /* == Args decoder */
  size_t offset = 0;
  uint8_t* args = tx_ctx->args;

  /**
   * args[0..8] magic header + call kind
   * Only access native eth_address after Polyjuice v1.0.0
   */
  static const uint8_t eth_polyjuice_args_header[7]
    = {0xff, 0xff, 0xff, 'P', 'O', 'L', 'Y'};
  if (memcmp(eth_polyjuice_args_header, args, 7) != 0) {
    debug_print_data("invalid polyjuice args header", args, 7);
    return -1;
  }
  debug_print_int("[call_kind]", args[7]);
  if (args[7] != EVMC_CALL && args[7] != EVMC_CREATE) {
    ckb_debug("invalid call kind");
    return -1;
  }
  evmc_call_kind kind = (evmc_call_kind)args[7];
  offset += 8;

  /* args[8..16] gas limit  */
  int64_t gas_limit;
  memcpy(&gas_limit, args + offset, sizeof(int64_t));
  offset += 8;
  debug_print_int("[gas_limit]", gas_limit);

  /* args[16..32] gas price */
  memcpy(&g_gas_price, args + offset, sizeof(uint128_t));
  offset += 16;
  debug_print_int("[gas_price]", (int64_t)(g_gas_price));

  /* args[32..48] transfer value */
  evmc_uint256be value{0};
  for (size_t i = 0; i < 16; i++) {
    value.bytes[31 - i] = args[offset + i];
  }
  offset += 16;

  /* args[48..52] */
  uint32_t input_size = *((uint32_t*)(args + offset));
  offset += 4;
  debug_print_int("[input_size]", input_size);

  if (input_size > tx_ctx->args_len) {
    /* If input size large enough may overflow `input_size + offset` */
    ckb_debug("input_size too large");
    return -1;
  }

  /* args[52..52+input_size] */
  uint8_t* input_data = args + offset;
  offset += input_size;
 
  if (offset + 20 == tx_ctx->args_len) { // This is a transfer tx.
    if (kind != EVMC_CALL) {
        ckb_debug("Native token transfer transaction only accepts CALL.");
        return -1;
    }
    g_eoa_transfer_flag = true;
    memcpy(g_eoa_transfer_to_address.bytes, args + offset, 20);
  } else if (offset != tx_ctx->args_len) {
    ckb_debug("invalid polyjuice transaction");
    return -1;
  }

  msg->kind = kind;
  msg->flags = 0;
  msg->depth = 0;
  msg->value = value;
  msg->input_data = input_data;
  msg->input_size = input_size;
  msg->gas = gas_limit;
  msg->sender = evmc_address{0};
  msg->destination = evmc_address{0};
  msg->create2_salt = evmc_bytes32{};
  return 0;
}

void release_result(const struct evmc_result* result) {
  if (result->output_data != NULL) {
    free((void*)result->output_data);
  }
  return;
}

int load_account_code(gw_context_t* gw_ctx, uint32_t account_id,
                      uint64_t* code_size, uint64_t offset, uint8_t* code) {

  int ret;
  uint8_t buffer[GW_MAX_SCRIPT_SIZE];
  mol_seg_t script_seg;
  ret = load_account_script(gw_ctx, account_id, buffer, GW_MAX_SCRIPT_SIZE, &script_seg);
  if (ret == GW_ERROR_ACCOUNT_NOT_EXISTS) {
    // This is an EoA or other kind of account, and not yet created
    debug_print_int("account not found", account_id);
    *code_size = 0;
    return 0;
  }
  if (ret != 0) {
    return ret;
  }
  mol_seg_t code_hash_seg = MolReader_Script_get_code_hash(&script_seg);
  mol_seg_t hash_type_seg = MolReader_Script_get_hash_type(&script_seg);
  mol_seg_t args_seg = MolReader_Script_get_args(&script_seg);
  mol_seg_t raw_args_seg = MolReader_Bytes_raw_bytes(&args_seg);
  if (raw_args_seg.size != CONTRACT_ACCOUNT_SCRIPT_ARGS_LEN) {
    debug_print_int("[load_account_code] invalid account script", account_id);
    debug_print_int("[load_account_code] raw_args_seg.size", raw_args_seg.size);
    // This is an EoA or other kind of account
    *code_size = 0;
    return 0;
  }
  if (memcmp(code_hash_seg.ptr, g_script_code_hash, 32) != 0
      || *hash_type_seg.ptr != g_script_hash_type
      /* compare rollup_script_hash */
      || memcmp(raw_args_seg.ptr, g_rollup_script_hash, 32) != 0
      /* compare creator account id */
      || memcmp(&g_creator_account_id, raw_args_seg.ptr + 32, sizeof(uint32_t)) != 0
  ) {
    debug_print_int("[load_account_code] creator account id not match for account", account_id);
    // This is an EoA or other kind of account
    *code_size = 0;
    return 0;
  }

  debug_print_int("[load_account_code] account_id", account_id);
  uint8_t key[32];
  uint8_t data_hash[32];
  polyjuice_build_contract_code_key(account_id, key);
  ret = gw_ctx->sys_load(gw_ctx, account_id, key, GW_KEY_BYTES, data_hash);
  if (ret != 0) {
    debug_print_int("[load_account_code] sys_load failed", ret);
    return ret;
  }

  bool is_data_hash_zero = true;
  for (size_t i = 0; i < 32; i++) {
    if (data_hash[i] != 0) {
      is_data_hash_zero = false;
      break;
    }
  }
  if (is_data_hash_zero) {
    ckb_debug("[load_account_code] data hash all zero");
    *code_size = 0;
    return 0;
  }

  debug_print_int("[load_account_code] code_size before loading", *code_size);
  ret = gw_ctx->sys_load_data(gw_ctx, data_hash, code_size, offset, code);
  debug_print_int("[load_account_code] code_size after loading", *code_size);
  if (ret != 0) {
    ckb_debug("[load_account_code] sys_load_data failed");
    return ret;
  }
  if (*code_size > MAX_DATA_SIZE) {
    debug_print_int("[load_account_code] code_size can't be larger than",
                    MAX_DATA_SIZE);
    return GW_FATAL_BUFFER_OVERFLOW;
  }

  return 0;
}

////////////////////////////////////////////////////////////////////////////////
//// Callbacks - EVMC Host Interfaces
////////////////////////////////////////////////////////////////////////////////
struct evmc_tx_context get_tx_context(struct evmc_host_context* context) {
  struct evmc_tx_context ctx{0};
  memcpy(ctx.tx_origin.bytes, g_tx_origin.bytes, 20);
  evmc_uint256be gas_price = {0};
  uint8_t* gas_price_ptr = (uint8_t*)(&g_gas_price);
  for (int i = 0; i < 16; i++) {
    gas_price.bytes[31 - i] = *(gas_price_ptr + i);
  }
  ctx.tx_gas_price = gas_price;

  ctx.block_number = context->gw_ctx->block_info.number;
  /*
    block_timestamp      => second
    block_info.timestamp => millisecond
  */
  ctx.block_timestamp = context->gw_ctx->block_info.timestamp / 1000;
  /* Ethereum block gas limit */
  ctx.block_gas_limit = 12500000;
  /* 2500000000000000 */
  ctx.block_difficulty = {
      0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
      0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
      0x00, 0x00, 0x00, 0x08, 0xe1, 0xbc, 0x9b, 0xf0, 0x40, 0x00,
  };

 uint8_t *chain_id_ptr = (uint8_t *)(&g_chain_id);
  ctx.chain_id.bytes[31] = chain_id_ptr[0];
  ctx.chain_id.bytes[30] = chain_id_ptr[1];
  ctx.chain_id.bytes[29] = chain_id_ptr[2];
  ctx.chain_id.bytes[28] = chain_id_ptr[3];
  ctx.chain_id.bytes[27] = chain_id_ptr[4];
  ctx.chain_id.bytes[26] = chain_id_ptr[5];
  ctx.chain_id.bytes[25] = chain_id_ptr[6];
  ctx.chain_id.bytes[24] = chain_id_ptr[7];

  /* block_coinbase */
  gw_reg_addr_t *block_producer = &context->gw_ctx->block_info.block_producer;
  if (block_producer->reg_id != GW_DEFAULT_ETH_REGISTRY_ACCOUNT_ID
   || block_producer->addr_len != ETH_ADDRESS_LEN) {
    ckb_debug("[get_tx_context] Error: block_producer is not an Ethereum EOA.");
    ckb_debug("[get_tx_context] failed to load block_coinbase address");
    context->error_code = GW_FATAL_INVALID_CONTEXT;
  } else {
    debug_print_data("load block_coinbase eth_address:",
                     block_producer->addr, ETH_ADDRESS_LEN);
    memcpy(ctx.block_coinbase.bytes, block_producer->addr, ETH_ADDRESS_LEN);
  }

  return ctx;
}

bool account_exists(struct evmc_host_context* context,
                    const evmc_address* address) {
  debug_print_data("BEGIN account_exists", address->bytes, 20);
  uint8_t script_hash[32] = {0};
  bool exists = true;
  int ret = load_script_hash_by_eth_address(context->gw_ctx, address->bytes,
                                            script_hash);
  if (ret != 0) {
    exists = false;
    debug_print_int("[account_exists] load_script_hash_by_eth_address failed",
                    ret);
  }
  debug_print_int("END account_exists", (int)exists);
  return exists;
}

evmc_bytes32 get_storage(struct evmc_host_context* context,
                         const evmc_address* address, const evmc_bytes32* key) {
  ckb_debug("BEGIN get_storage");
  evmc_bytes32 value{0};
  int ret = context->gw_ctx->sys_load(context->gw_ctx, context->to_id,
                                      key->bytes, GW_KEY_BYTES,
                                      (uint8_t *)value.bytes);
  if (ret != 0) {
    debug_print_int("get_storage, sys_load failed", ret);
    if (is_fatal_error(ret)) {
      context->error_code = ret;
    }
  }
  ckb_debug("END get_storage");
  return value;
}

enum evmc_storage_status set_storage(struct evmc_host_context* context,
                                     const evmc_address* address,
                                     const evmc_bytes32* key,
                                     const evmc_bytes32* value) {
  ckb_debug("BEGIN set_storage");
  evmc_storage_status status = EVMC_STORAGE_ADDED;
  int ret = context->gw_ctx->sys_store(context->gw_ctx, context->to_id,
                                       key->bytes, GW_KEY_BYTES, value->bytes);
  if (ret != 0) {
    debug_print_int("sys_store failed", ret);
    if (is_fatal_error(ret)) {
      context->error_code = ret;
    }
    status = EVMC_STORAGE_UNCHANGED;
  }
  /* TODO: more rich evmc_storage_status */
  ckb_debug("END set_storage");
  return status;
}

size_t get_code_size(struct evmc_host_context* context,
                     const evmc_address* address) {
  ckb_debug("BEGIN get_code_size");
  uint32_t account_id = 0;
  int ret = load_account_id_by_eth_address(context->gw_ctx,
                                           address->bytes, &account_id);
  if (ret == GW_ERROR_NOT_FOUND) {
    ckb_debug("END get_code_size");
    return 0;
  }
  if (ret != 0) {
    ckb_debug("get contract account id failed");
    return 0;
  }

  uint8_t code[MAX_DATA_SIZE];
  uint64_t code_size = MAX_DATA_SIZE;
  ret = load_account_code(context->gw_ctx, account_id, &code_size, 0, code);
  if (ret != 0) {
    debug_print_int("[get_code_size] load_account_code failed", ret);
    context->error_code = ret;
    return 0;
  }

  ckb_debug("END get_code_size");
  return code_size;
}

evmc_bytes32 get_code_hash(struct evmc_host_context* context,
                           const evmc_address* address) {
  ckb_debug("BEGIN get_code_hash");
  evmc_bytes32 hash{0};
  uint32_t account_id = 0;
  int ret = load_account_id_by_eth_address(context->gw_ctx,
                                           address->bytes, &account_id);
  if (ret == GW_ERROR_NOT_FOUND) {
    ckb_debug("END get_code_hash");
    return hash;
  }
  if (ret != 0) {
    ckb_debug("get contract account id failed");
    context->error_code = ret;
    return hash;
  }

  uint8_t code[MAX_DATA_SIZE];
  uint64_t code_size = MAX_DATA_SIZE;
  ret = load_account_code(context->gw_ctx, account_id, &code_size, 0, code);
  if (ret != 0) {
    debug_print_int("[get_code_hash] load_account_code failed", ret);
    context->error_code = ret;
    return hash;
  }

  if (code_size > 0) {
    union ethash_hash256 hash_result = ethash::keccak256(code, code_size);
    memcpy(hash.bytes, hash_result.bytes, 32);
  }
  ckb_debug("END get_code_hash");
  return hash;
}

/**
 * @brief Copy code callback function.
 *
 * This callback function is used by an EVM to request a copy of the code of the
 * given account to the memory buffer provided by the EVM. The Client MUST copy
 * the requested code, starting with the given offset, to the provided memory
 * buffer up to the size of the buffer or the size of the code, whichever is
 * smaller.
 *
 * @param context The pointer to the Host execution context.
 * @param address The address of the account.
 * @param code_offset The offset of the code to copy.
 * @param buffer_data The pointer to the memory buffer allocated by the EVM to
 *                    store a copy of the requested code.
 * @param buffer_size The size of the memory buffer.
 * @return size_t The number of bytes copied to the buffer by the Client.
 */
size_t copy_code(struct evmc_host_context* context, const evmc_address* address,
                 size_t code_offset, uint8_t* buffer_data, size_t buffer_size) {
  ckb_debug("BEGIN copy_code");
  debug_print_int("[copy_code] code_offset", code_offset);
  debug_print_int("[copy_code] buffer_size", buffer_size);
  uint32_t account_id = 0;
  int ret = load_account_id_by_eth_address(context->gw_ctx,
                                           address->bytes, &account_id);
  if (ret == GW_ERROR_NOT_FOUND) {
    ckb_debug("END copy_code");
    return 0;
  }
  if (ret != 0) {
    ckb_debug("get contract account id failed");
    context->error_code = ret;
    return 0;
  }

  uint64_t code_size = buffer_size;
  ret = load_account_code(context->gw_ctx, account_id, &code_size,
                          code_offset, buffer_data);
  if (ret != 0) {
    debug_print_int("[copy_code] load_account_code failed", ret);
    context->error_code = ret;
    return 0;
  }

  ckb_debug("END copy_code");
  return code_size >= buffer_size ? buffer_size : code_size;
}

evmc_uint256be get_balance(struct evmc_host_context* context,
                           const evmc_address* address) {
  ckb_debug("BEGIN get_balance");
  evmc_uint256be balance{};

  gw_reg_addr_t addr = new_reg_addr(address->bytes);

  uint256_t value = {0};
  int ret = sudt_get_balance(context->gw_ctx,
                             g_sudt_id, /* g_sudt_id account must exists */
                             addr, &value);
  if (ret != 0) {
    ckb_debug("sudt_get_balance failed");
    context->error_code = FATAL_POLYJUICE;
    return balance;
  }

  uint8_t* value_ptr = (uint8_t*)(&value);
  for (int i = 0; i < 32; i++) {
    balance.bytes[31 - i] = *(value_ptr + i);
  }
  debug_print_data("address", address->bytes, 20);
  debug_print_data("balance", (uint8_t*)&value, 32);
  ckb_debug("END get_balance");
  return balance;
}

void selfdestruct(struct evmc_host_context* context,
                  const evmc_address* address,
                  const evmc_address* beneficiary) {
  gw_reg_addr_t from_addr = new_reg_addr(address->bytes);

  uint256_t balance;
  int ret = sudt_get_balance(context->gw_ctx,
                             g_sudt_id, /* g_sudt_id account must exists */
                             from_addr, &balance);
  if (ret != 0) {
    ckb_debug("get balance failed");
    context->error_code = ret;
    return;
  }

  uint256_t zero = {0};
  if (gw_uint256_cmp(balance, zero) == GW_UINT256_LARGER) {
    gw_reg_addr_t to_addr = new_reg_addr(beneficiary->bytes);

    ret = sudt_transfer(context->gw_ctx, g_sudt_id,
                        from_addr,
                        to_addr,
                        balance);
    if (ret != 0) {
      ckb_debug("transfer beneficiary failed");
      context->error_code = ret;
      return;
    }
  }

  uint8_t raw_key[GW_KEY_BYTES];
  uint8_t value[GW_VALUE_BYTES];
  polyjuice_build_destructed_key(context->to_id, raw_key);
  memset(value, 1, GW_VALUE_BYTES);
  ret = context->gw_ctx->_internal_store_raw(context->gw_ctx, raw_key, value);
  if (ret != 0) {
    ckb_debug("update selfdestruct special key failed");
    context->error_code = ret;
  }
  ckb_debug("END selfdestruct");
  return;
}

struct evmc_result call(struct evmc_host_context* context,
                        const struct evmc_message* msg) {
  ckb_debug("BEGIN call");
  debug_print_int("msg.gas", msg->gas);
  debug_print_int("msg.depth", msg->depth);
  debug_print_int("msg.kind", msg->kind);
  debug_print_data("call.sender", msg->sender.bytes, 20);
  debug_print_data("call.destination", msg->destination.bytes, 20);
  int ret;
  struct evmc_result res;
  memset(&res, 0, sizeof(res));
  res.release = release_result;
  gw_context_t* gw_ctx = context->gw_ctx;

  /*
   * Take a snapshot for call and revert later if EVM returns an error.
   */
  uint32_t snapshot_id;
  ret = gw_ctx->sys_snapshot(gw_ctx, &snapshot_id);
  debug_print_int("[call] take a snapshot", snapshot_id);
  if (ret != 0) {
    res.status_code = EVMC_INTERNAL_ERROR;
    return res;
  }

  precompiled_contract_gas_fn contract_gas;
  precompiled_contract_fn contract;
  if (match_precompiled_address(&msg->destination, &contract_gas, &contract)) {
    uint64_t gas_cost = 0;
    ret = contract_gas(msg->input_data, msg->input_size, &gas_cost);
    if (is_fatal_error(ret)) {
      context->error_code = ret;
    }
    if (ret != 0) {
      ckb_debug("call pre-compiled contract gas failed");
      res.status_code = EVMC_INTERNAL_ERROR;
      return res;
    }
    if ((uint64_t)msg->gas < gas_cost) {
      ckb_debug("call pre-compiled contract out of gas");
      res.status_code = EVMC_OUT_OF_GAS;
      return res;
    }
    res.gas_left = msg->gas - (int64_t)gas_cost;
    ret = contract(gw_ctx,
                   context->code_data, context->code_size,
                   context->kind,
                   msg->flags == EVMC_STATIC,
                   msg->input_data, msg->input_size,
                   (uint8_t**)&res.output_data, &res.output_size);
    if (is_fatal_error(ret)) {
      context->error_code = ret;
    }
    if (ret != 0) {
      debug_print_int("call pre-compiled contract failed", ret);
      res.status_code = EVMC_INTERNAL_ERROR;
      int revert_ret = gw_ctx->sys_revert(gw_ctx, snapshot_id);
      debug_print_int("[call precompiled] revert with snapshot id", snapshot_id);
      if (is_fatal_error(revert_ret)) {
        context->error_code = ret;
      }
      return res;
    }
    res.status_code = EVMC_SUCCESS;
  } else {
    ret = handle_message(gw_ctx, context->from_id, context->to_id,
                         &context->destination, msg, &res);
    if (res.status_code != EVMC_SUCCESS) {
      int revert_ret = gw_ctx->sys_revert(gw_ctx, snapshot_id);
      debug_print_int("[call] revert with snapshot id", snapshot_id);
      if (is_fatal_error(revert_ret)) {
        context->error_code = ret;
      }
    }
    if (is_fatal_error(ret)) {
      /* stop as soon as possible */
      context->error_code = ret;
    }
    if (ret != 0) {
      debug_print_int("inner call failed (transfer/contract call contract)", ret);
      if (is_evmc_error(ret)) {
        res.status_code = (evmc_status_code)ret;
      } else {
        res.status_code = EVMC_INTERNAL_ERROR;
      }
    }
  }
  debug_print_int("call.res.status_code", res.status_code);
  ckb_debug("END call");

  return res;
}

evmc_bytes32 get_block_hash(struct evmc_host_context* context, int64_t number) {
  ckb_debug("BEGIN get_block_hash");
  evmc_bytes32 block_hash{};
  int ret = context->gw_ctx->sys_get_block_hash(context->gw_ctx, number,
                                                (uint8_t*)block_hash.bytes);
  if (ret != 0) {
    ckb_debug("sys_get_block_hash failed");
    context->error_code = ret;
    return block_hash;
  }
  ckb_debug("END get_block_hash");
  return block_hash;
}

void emit_log(struct evmc_host_context* context, const evmc_address* address,
              const uint8_t* data, size_t data_size,
              const evmc_bytes32 topics[], size_t topics_count) {
  ckb_debug("BEGIN emit_log");
  /*
    output[ 0..20]                     = callee_contract.address
    output[20..24]                     = data_size_u32
    output[24..24+data_size]           = data
    ouptut[24+data_size..28+data_size] = topics_count_u32
    ouptut[28+data_size..]             = topics
   */
  size_t output_size = 20 + (4 + data_size) + (4 + topics_count * 32);
  uint8_t* output = (uint8_t*)malloc(output_size);
  if (output == NULL) {
    context->error_code = -1;
    return;
  }
  uint32_t data_size_u32 = (uint32_t)(data_size);
  uint32_t topics_count_u32 = (uint32_t)(topics_count);

  uint8_t* output_current = output;
  memcpy(output_current, address->bytes, 20);
  output_current += 20;
  memcpy(output_current, (uint8_t*)(&data_size_u32), 4);
  output_current += 4;
  if (data_size > 0) {
    memcpy(output_current, data, data_size);
    output_current += data_size;
  }
  memcpy(output_current, (uint8_t*)(&topics_count_u32), 4);
  output_current += 4;
  for (size_t i = 0; i < topics_count; i++) {
    debug_print_data("log.topic", topics[i].bytes, 32);
    memcpy(output_current, topics[i].bytes, 32);
    output_current += 32;
  }
  int ret = context->gw_ctx->sys_log(context->gw_ctx, context->to_id,
                                     GW_LOG_POLYJUICE_USER, (uint32_t)output_size, output);
  if (ret != 0) {
    ckb_debug("sys_log failed");
    context->error_code = ret;
  }
  free(output);
  ckb_debug("END emit_log");
  return;
}

/**
 * check address collision
 * check existence of eth_addr
 * If it's an EoA address with non-zero nonce or it's an contract address, it can't be overwrite.
 * @param overwrite true if there is a collision but we can continue to create a new account
 * @return 0 means success
*/
int check_address_collision(gw_context_t* ctx, const uint8_t eth_addr[ETH_ADDRESS_LEN], bool* overwrite) {
  gw_reg_addr_t addr = new_reg_addr(eth_addr);
  uint8_t script_hash[32] = {0};
  int ret = ctx->sys_get_script_hash_by_registry_address(ctx, &addr, script_hash);
  if (ret == GW_ERROR_NOT_FOUND) {
    return 0;
  }
  if (ret != 0) {
    return ret;
  }
  uint32_t account_id;
  ret = ctx->sys_get_account_id_by_script_hash(ctx, script_hash, &account_id);
  if (ret != 0) {
    return ret;
  }
  // account exists
  uint32_t nonce;
  ret = ctx->sys_get_account_nonce(ctx, account_id, &nonce);
  if (ret != 0) {
    return ret;
  }
  uint8_t code[MAX_DATA_SIZE];
  uint64_t code_size = MAX_DATA_SIZE;
  ret = load_account_code(ctx, account_id, &code_size, 0, code);
  if (ret != 0) {
    return ret;
  }
  // check nonce and EOA
  if (nonce > 0 || code_size > 0) {
    return ERROR_CONTRACT_ADDRESS_COLLISION;
  }
  // There is a collision. We can create a new account and re-map.
  *overwrite = true;
  
  ckb_debug("[address collision] continue and re-map");
  return 0;
}
/**
 * @return 0 if the `to_id` account is not destructed
 */
int check_destructed(gw_context_t* ctx, uint32_t to_id) {
  int ret;
  uint8_t destructed_raw_key[GW_KEY_BYTES];
  uint8_t destructed_raw_value[GW_VALUE_BYTES] = {0};
  polyjuice_build_destructed_key(to_id, destructed_raw_key);
  ret = ctx->_internal_load_raw(ctx, destructed_raw_key, destructed_raw_value);
  if (ret != 0) {
    debug_print_int("load destructed key failed", ret);
    return ret;
  }
  bool destructed = true;
  for (int i = 0; i < GW_VALUE_BYTES; i++) {
    if (destructed_raw_value[i] == 0) {
      destructed = false;
      break;
    }
  }
  if (destructed) {
    ckb_debug("call a contract that was already destructed");
    return FATAL_POLYJUICE;
  }
  return 0;
}

/**
 * load the following global values:
 * - g_chain_id
 * - g_creator_account_id
 * - g_script_hash_type
 * - g_rollup_script_hash
 * - g_sudt_id
 */
int load_globals(gw_context_t* ctx, uint32_t to_id) {
  uint8_t buffer[GW_MAX_SCRIPT_SIZE];
  mol_seg_t script_seg;
  int ret = load_account_script(ctx, to_id, buffer, GW_MAX_SCRIPT_SIZE, &script_seg);
  if (ret != 0) {
    return ret;
  }
  mol_seg_t code_hash_seg = MolReader_Script_get_code_hash(&script_seg);
  mol_seg_t hash_type_seg = MolReader_Script_get_hash_type(&script_seg);
  mol_seg_t args_seg = MolReader_Script_get_args(&script_seg);
  mol_seg_t raw_args_seg = MolReader_Bytes_raw_bytes(&args_seg);

  memcpy(g_script_code_hash, code_hash_seg.ptr, 32);
  g_script_hash_type = *hash_type_seg.ptr;

  uint8_t creator_script_buffer[GW_MAX_SCRIPT_SIZE];
  mol_seg_t creator_script_seg;
  mol_seg_t creator_raw_args_seg;
  if (raw_args_seg.size == CREATOR_SCRIPT_ARGS_LEN) {
    /* polyjuice creator account */
    g_creator_account_id = to_id;
    creator_raw_args_seg = raw_args_seg;
  } else if (raw_args_seg.size == CONTRACT_ACCOUNT_SCRIPT_ARGS_LEN) {
    /* read creator account id and do some checking */
    memcpy(&g_creator_account_id, raw_args_seg.ptr + 32, sizeof(uint32_t));
    int ret = load_account_script(ctx,
                                  g_creator_account_id,
                                  creator_script_buffer,
                                  GW_MAX_SCRIPT_SIZE,
                                  &creator_script_seg);
    if (ret != 0) {
      return ret;
    }
    mol_seg_t creator_code_hash_seg = MolReader_Script_get_code_hash(&creator_script_seg);
    mol_seg_t creator_hash_type_seg = MolReader_Script_get_hash_type(&creator_script_seg);
    mol_seg_t creator_args_seg = MolReader_Script_get_args(&creator_script_seg);
    creator_raw_args_seg = MolReader_Bytes_raw_bytes(&creator_args_seg);
    if (memcmp(creator_code_hash_seg.ptr, code_hash_seg.ptr, 32) != 0
        || *creator_hash_type_seg.ptr != *hash_type_seg.ptr
        /* compare rollup_script_hash */
        || memcmp(creator_raw_args_seg.ptr, raw_args_seg.ptr, 32) != 0
        || creator_raw_args_seg.size != CREATOR_SCRIPT_ARGS_LEN) {
      debug_print_int("invalid creator account id in normal contract account script args",
                      g_creator_account_id);
      return FATAL_POLYJUICE;
    }
  } else {
    debug_print_data("invalid to account script args", raw_args_seg.ptr, raw_args_seg.size);
    return FATAL_POLYJUICE;
  }
  /** read g_chain_id from Godwoken RollupConfig */
  mol_seg_t rollup_config_seg;
  rollup_config_seg.ptr = ctx->rollup_config;
  rollup_config_seg.size = ctx->rollup_config_size;
  mol_seg_t id_u64_seg = MolReader_RollupConfig_get_chain_id(&rollup_config_seg);
  memcpy(&g_chain_id, id_u64_seg.ptr, id_u64_seg.size);
  debug_print_int("chain_id", g_chain_id);
  
  debug_print_int("creator_account_id", g_creator_account_id);

  /** read rollup_script_hash and g_sudt_id from creator account */
  memcpy(g_rollup_script_hash, creator_raw_args_seg.ptr, 32);
  memcpy(&g_sudt_id, creator_raw_args_seg.ptr + 32, sizeof(uint32_t));
  debug_print_data("g_rollup_script_hash", g_rollup_script_hash, 32);
  debug_print_int("g_sudt_id", g_sudt_id);

  return 0;
}

int create_new_account(gw_context_t* ctx,
                       const evmc_message* msg,
                       uint32_t from_id,
                       uint32_t* to_id,
                       uint8_t* code_data,
                       size_t code_size) {
  if (code_size == 0) {
    ckb_debug("[create_new_account] can't create new account by empty code data");
    return FATAL_POLYJUICE;
  }

  int ret = 0;
  uint8_t script_args[CONTRACT_ACCOUNT_SCRIPT_ARGS_LEN];
  uint8_t data[128] = {0};
  uint32_t data_len = 0;
  if (msg->kind == EVMC_CREATE) {
    /* normal contract account script.args[36..36+20] content before hash
       Include:
       - [20 bytes] sender address
       - [4  bytes] sender nonce (NOTE: only use first 4 bytes (u32))

       Above data will be RLP encoded.
    */
    ckb_debug("[create_new_account] msg->kind == EVMC_CREATE");
    uint32_t nonce;
    /* from_id must already exists */
    ret = ctx->sys_get_account_nonce(ctx, from_id, &nonce);
    if (ret != 0) {
      return ret;
    }
    debug_print_data("sender", msg->sender.bytes, 20);
    debug_print_int("from_id", from_id);
    debug_print_int("nonce", nonce);
    rlp_encode_sender_and_nonce(&msg->sender, nonce, data, &data_len);
  } else if (msg->kind == EVMC_CREATE2) {
    /* CREATE2 contract account script.args[36..36+20] content before hash
       Include:
       - [ 1 byte ] 0xff (refer to ethereum)
       - [20 bytes] sender address
       - [32 bytes] create2_salt
       - [32 bytes] keccak256(init_code)
    */
    ckb_debug("[create_new_account] msg->kind == EVMC_CREATE2");
    union ethash_hash256 hash_result = ethash::keccak256(code_data, code_size);
    data[0] = 0xff;
    memcpy(data + 1, msg->sender.bytes, 20);
    memcpy(data + 1 + 20, msg->create2_salt.bytes, 32);
    memcpy(data + 1 + 20 + 32, hash_result.bytes, 32);
    data_len = 1 + 20 + 32 + 32;
  } else {
    ckb_debug("[create_new_account] unreachable");
    return FATAL_POLYJUICE;
  }

  /* contract account script.args
     Include:
     - [32 bytes] rollup type hash
     - [ 4 bytes] little endian creator_account_id, it's Polyjuice Root Account
     - [20 bytes] keccak256(data)[12..]
  */
  memcpy(script_args, g_rollup_script_hash, 32);
  memcpy(script_args + 32, (uint8_t*)(&g_creator_account_id), 4);
  union ethash_hash256 data_hash_result = ethash::keccak256(data, data_len);
  uint8_t *eth_addr = data_hash_result.bytes + 12;
  memcpy(script_args + 32 + 4, eth_addr, ETH_ADDRESS_LEN);

  bool overwrite = false;
  ret = check_address_collision(ctx, eth_addr, &overwrite);
  if (ret != 0) {
    return ret;
  }

  mol_seg_t new_script_seg;
  uint32_t new_account_id;
  ret = build_script(g_script_code_hash, g_script_hash_type, script_args,
                     CONTRACT_ACCOUNT_SCRIPT_ARGS_LEN, &new_script_seg);
  if (ret != 0) {
    return ret;
  }
  uint8_t script_hash[32];
  blake2b_hash(script_hash, new_script_seg.ptr, new_script_seg.size);
  ret = ctx->sys_create(ctx, new_script_seg.ptr, new_script_seg.size, &new_account_id);
  if (ret != 0) {
    debug_print_int("sys_create error", ret);

    // create account failed assume account already created by meta_contract
    ret = ctx->sys_get_account_id_by_script_hash(ctx, script_hash, &new_account_id);
    if (ret != 0) {
      return ret;
    }
  }
  free(new_script_seg.ptr);
  *to_id = new_account_id;
  memcpy((uint8_t *)msg->destination.bytes, eth_addr, 20);
  debug_print_int(">> new to id", *to_id);

  // register a created contract account into `ETH Address Registry`
  ret = gw_update_eth_address_register(ctx, eth_addr, script_hash, overwrite);
  if (ret != 0) {
    ckb_debug("[create_new_account] failed to register a contract account");
    return ret;
  }

  return 0;
}

int handle_transfer(gw_context_t* ctx,
                    const evmc_message* msg,
                    bool to_address_is_eoa) {
  uint256_t value;
  uint8_t* value_ptr = (uint8_t*)&value;
  for (int i = 0; i < 32; i++) {
    value_ptr[i] = msg->value.bytes[31 - i];
  }
  debug_print_data("[handle_transfer] sender", msg->sender.bytes, 20);
  debug_print_data("[handle_transfer] destination", msg->destination.bytes, 20);
  debug_print_data("[handle_transfer] msg->value", (uint8_t*)&value, 32);

  if (msg->kind == EVMC_CALL
   && memcmp(msg->sender.bytes, g_tx_origin.bytes, 20) == 0
   && to_address_is_eoa) {
    ckb_debug("warning: transfer value from eoa to eoa");
    return FATAL_POLYJUICE;
  }

  gw_reg_addr_t from_addr = new_reg_addr(msg->sender.bytes);
  gw_reg_addr_t to_addr = new_reg_addr(msg->destination.bytes);

  uint256_t zero = {0};
  if (gw_uint256_cmp(value, zero) == GW_UINT256_EQUAL) {
    return 0;
  }
  int ret = sudt_transfer(ctx, g_sudt_id, from_addr, to_addr, value);
  if (ret != 0) {
    ckb_debug("[handle_transfer] sudt_transfer failed");
    return ret;
  }

  return 0;
}

int load_eth_eoa_type_hash(gw_context_t* ctx, uint8_t eoa_type_hash[GW_KEY_BYTES]) {
  mol_seg_t rollup_config_seg;
  rollup_config_seg.ptr = ctx->rollup_config;
  rollup_config_seg.size = ctx->rollup_config_size;

  mol_seg_t allowed_eoa_list_seg =
      MolReader_RollupConfig_get_allowed_eoa_type_hashes(&rollup_config_seg);
  uint32_t len = MolReader_AllowedTypeHashVec_length(&allowed_eoa_list_seg);
  for (uint32_t i = 0; i < len; i++) {
    mol_seg_res_t allowed_type_hash_res =
        MolReader_AllowedTypeHashVec_get(&allowed_eoa_list_seg, i);

    if (!is_errno_ok(&allowed_type_hash_res)) {
      return GW_FATAL_INVALID_DATA;
    }
    mol_seg_t type_seg =
        MolReader_AllowedTypeHash_get_type_(&allowed_type_hash_res.seg);
    if (*(uint8_t *)type_seg.ptr == GW_ALLOWED_EOA_ETH) {
      mol_seg_t eth_lock_code_hash_seg =
          MolReader_AllowedTypeHash_get_hash(&allowed_type_hash_res.seg);
      memcpy(eoa_type_hash, eth_lock_code_hash_seg.ptr, 32);
      return 0;
    }
  }
  ckb_debug("Cannot find EoA type hash of ETH.");
  return FATAL_POLYJUICE;
 }

int handle_native_token_transfer(gw_context_t* ctx, uint32_t from_id,
                                 uint256_t value, gw_reg_addr_t* from_addr,
                                 uint64_t* gas_used) {
  if (g_creator_account_id == UINT32_MAX) {
    ckb_debug("[handle_native_token_transfer] g_creator_account_id wasn't set.");
    return ERROR_NATIVE_TOKEN_TRANSFER;
  }
  if (!g_eoa_transfer_flag) {
    ckb_debug("[handle_native_token_transfer] not a native transfer tx");
    return ERROR_NATIVE_TOKEN_TRANSFER;
  }

  int ret = 0;
  uint8_t from_script_hash[GW_KEY_BYTES] = {0};
  ret = ctx->sys_get_script_hash_by_account_id(ctx, from_id, from_script_hash);
  if (ret != 0) {
    return ret;
  }
  ret = ctx->sys_get_registry_address_by_script_hash(ctx, from_script_hash,
                                                     GW_DEFAULT_ETH_REGISTRY_ACCOUNT_ID,
                                                     from_addr);
  if (ret != 0) {
    return ret;
  }

  gw_reg_addr_t to_addr = new_reg_addr(g_eoa_transfer_to_address.bytes);
  // check to_addr is not a contract
  uint8_t to_script_hash[GW_KEY_BYTES] = {0};
  ret = ctx->sys_get_script_hash_by_registry_address(ctx, &to_addr, to_script_hash);
  if (ret == 0) {
    uint32_t to_id;
    ret = ctx->sys_get_account_id_by_script_hash(ctx, to_script_hash, &to_id);
    if (ret != 0) {
        return ret;
    }

    uint8_t code[MAX_DATA_SIZE];
    uint64_t code_size = MAX_DATA_SIZE;
    ret = load_account_code(ctx, to_id, &code_size, 0, code);
    if (ret != 0) {
      return ret;
    }
    // to address is a contract
    if (code_size > 0) {
      ckb_debug("[handle_native_token_transfer] to_address is a contract");
      return ERROR_NATIVE_TOKEN_TRANSFER;
    }
  } else if (ret == GW_ERROR_NOT_FOUND) {
    ckb_debug("[handle_native_token_transfer] create new EoA account");
    //build eoa script
    uint8_t eoa_type_hash[GW_KEY_BYTES] = {0};
    ret = load_eth_eoa_type_hash(ctx, eoa_type_hash);
    if (ret != 0) {
        return ret;
    }
    // EOA script args len: 32 + 20
    int eoa_script_args_len = 32 + 20;
    uint8_t script_args[eoa_script_args_len];
    memcpy(script_args, g_rollup_script_hash, 32);
    memcpy(script_args + 32, g_eoa_transfer_to_address.bytes, 20);
    mol_seg_t new_script_seg;
    ret = build_script(eoa_type_hash, g_script_hash_type, script_args,
                       eoa_script_args_len, &new_script_seg);
    if (ret != 0) {
      return ret;
    }
    uint32_t new_account_id;
    ret = ctx->sys_create(ctx, new_script_seg.ptr, new_script_seg.size,
                          &new_account_id);
    if (ret != 0) {
      ckb_debug("[handle_native_token_transfer] create new account failed.");
      return ret;
    }
    uint8_t account_script_hash[32] = {0};
    ret = ctx->sys_get_script_hash_by_account_id(ctx, new_account_id,
                                                 account_script_hash);
    if (ret != 0) {
      ckb_debug("[handle_native_token_transfer] failed to get created eth account script hash");
      return ret;
    }

    ret = gw_register_eth_address(ctx, account_script_hash);
    if (ret != 0) {
      ckb_debug("[handle_native_token_transfer] failed to register eth address");
      return ret;
    }
    // charge gas for new account
    *gas_used += NEW_ACCOUNT_GAS;
  } else {
    return ret;
  }

  ret = sudt_transfer(ctx, g_sudt_id, *from_addr, to_addr, value);
  if (ret != 0) {
    ckb_debug("[handle_native_token_transfer] sudt_transfer failed");
    return ret;
  }

  return 0;
}

int execute_in_evmone(gw_context_t* ctx,
                      evmc_message* msg,
                      uint32_t _parent_from_id,
                      uint32_t from_id,
                      uint32_t to_id,
                      const uint8_t* code_data,
                      const size_t code_size,
                      struct evmc_result* res) {
  int ret = 0;
  evmc_address sender = msg->sender;
  evmc_address destination = msg->destination;
  struct evmc_host_context context {ctx, code_data, code_size, msg->kind, from_id, to_id, sender, destination, 0};
  struct evmc_vm* vm = evmc_create_evmone();
  struct evmc_host_interface interface = {account_exists, get_storage,    set_storage,    get_balance,
                                          get_code_size,  get_code_hash,  copy_code,      selfdestruct,
                                          call,           get_tx_context, get_block_hash, emit_log};
  /* Execute the code in EVM */
  debug_print_int("[execute_in_evmone] code size", code_size);
  debug_print_int("[execute_in_evmone] input_size", msg->input_size);
  *res = vm->execute(vm, &interface, &context, EVMC_MAX_REVISION, msg, code_data, code_size);
  if (res->status_code != EVMC_SUCCESS && res->status_code != EVMC_REVERT) {
    res->output_data = NULL;
    res->output_size = 0;
  }
  if (context.error_code != 0) {
    debug_print_int("[execute_in_evmone] context.error_code", context.error_code);
    ret = context.error_code;
    goto evmc_vm_cleanup;
  }
  if (res->gas_left < 0) {
    ckb_debug("[execute_in_evmone] gas not enough");
    ret = EVMC_OUT_OF_GAS;
    goto evmc_vm_cleanup;
  }

evmc_vm_cleanup:
  evmc_destroy(vm); // destroy the VM instance
  return ret;
}

int store_contract_code(gw_context_t* ctx,
                        uint32_t to_id,
                        struct evmc_result* res) {
  int ret;
  uint8_t key[32];
  uint8_t data_hash[32];
  blake2b_hash(data_hash, (uint8_t*)res->output_data, res->output_size);
  polyjuice_build_contract_code_key(to_id, key);
  ckb_debug("BEGIN store data key");
  debug_print_data("code_data_hash", data_hash, 32);
  /* to_id must exists here */
  ret = ctx->sys_store(ctx, to_id, key, GW_KEY_BYTES, data_hash);
  if (ret != 0) {
    return ret;
  }
  ckb_debug("BEGIN store data");
  debug_print_int("contract_code_len", res->output_size);
  ret = ctx->sys_store_data(ctx, res->output_size, (uint8_t*)res->output_data);
  ckb_debug("END store data");
  if (ret != 0) {
    return ret;
  }
  return 0;
}

/**
 * call/create contract
 *
 * Must allocate an account id before create contract
 */
int handle_message(gw_context_t* ctx,
                   uint32_t parent_from_id,
                   uint32_t parent_to_id,
                   evmc_address *parent_destination,
                   const evmc_message* msg_origin,
                   struct evmc_result* res) {
  static const evmc_address zero_address{0};

  evmc_message msg = *msg_origin;
  int ret;

  bool to_address_exists = false;
  uint32_t to_id = 0;
  uint32_t from_id;

  if (memcmp(zero_address.bytes, msg.destination.bytes, 20) != 0) {
    ret = load_account_id_by_eth_address(ctx, msg.destination.bytes, &to_id);
    if (ret != 0) {
      debug_print_int(
        "[handle_message] load_account_id_by_eth_address failed", ret);
    } else {
      to_address_exists = true;
    }
  } else {
    /* When msg.destination is zero
        1. if is_create(msg.kind) == true, we will run msg.input_data as code in EVM
        2. if is_create(msg.kind) == false, code_size must be zero, so it's simply a transfer action
    */
  }

  /** get from_id */
  ret = load_account_id_by_eth_address(ctx, msg.sender.bytes, &from_id);
  if (ret != 0) {
    debug_print_int(
      "[handle_message] load_account_id_by_eth_address failed", ret);
    return ret;
  }

  /* an assert */
  if (msg.kind == EVMC_DELEGATECALL && from_id != parent_from_id) {
    debug_print_int("[handle_message] from_id", from_id);
    debug_print_int("[handle_message] parent_from_id", parent_from_id);
    ckb_debug("[handle_message] from id != parent from id");
    return FATAL_POLYJUICE;
  }

  /* Check if target contract is destructed */
  if (!is_create(msg.kind) && to_address_exists) {
    ret = check_destructed(ctx, to_id);
    if (ret != 0) {
      return ret;
    }
  }

  /* Load contract code from evmc_message or by sys_load_data */
  uint8_t* code_data = NULL;
  size_t code_size = 0;
  uint8_t code_data_buffer[MAX_DATA_SIZE];
  if (is_create(msg.kind)) {
    /* use input as code */
    code_data = (uint8_t*)msg.input_data;
    code_size = msg.input_size;
    msg.input_data = NULL;
    msg.input_size = 0;
  } else if (to_address_exists) {
    uint64_t code_size_tmp = MAX_DATA_SIZE;
    /* call kind: CALL/CALLCODE/DELEGATECALL */
    ret = load_account_code(ctx, to_id, &code_size_tmp, 0, code_data_buffer);
    if (ret != 0) {
      debug_print_int("[handle_message] load_account_code failed", ret);
      return ret;
    }
    if (code_size_tmp == 0) {
      debug_print_int("[handle_message] account with empty code (EoA account)",
                      to_id);
      code_data = NULL;
    } else {
      code_data = code_data_buffer;
    }
    code_size = (size_t)code_size_tmp;
  } else {
    /** Call non-exists address */
    ckb_debug("[handle_message] Warn: Call non-exists address");
  }

  /* Handle special call: CALLCODE/DELEGATECALL */
  if (is_special_call(msg.kind)) {
    /* This action must after load the contract code */
    to_id = parent_to_id;
    if (parent_destination == NULL) {
      ckb_debug("[handle_message] parent_destination is NULL");
      return FATAL_POLYJUICE;
    }
    memcpy(msg.destination.bytes, parent_destination->bytes, 20);
  }

  /* Create new account by script */
  /* NOTE: to_id may be rewritten */
  if (is_create(msg.kind)) {
    ret = create_new_account(ctx, &msg, from_id, &to_id, code_data, code_size);
    if (ret != 0) {
      return ret;
    }
    to_address_exists = true;

    /* It's a creation polyjuice transaction */
    if (parent_from_id == UINT32_MAX && parent_to_id == UINT32_MAX) {
      g_created_id = to_id;
      memcpy(g_created_address, msg.destination.bytes, 20);
    }

    /* Increase from_id's nonce:
         1. Must increase nonce after new address created and before run vm
         2. Only increase contract account's nonce when it create contract (https://github.com/ethereum/EIPs/blob/master/EIPS/eip-161.md)
     */
    ret = gw_increase_nonce(ctx, from_id, NULL);
    if (ret != 0) {
      debug_print_int("[handle_message] increase nonce failed", ret);
      return ret;
    }
  }

  /**
   * Handle transfer logic
   * 
   * NOTE:
   * 1. MUST do this before vm.execute and after to_id finalized
   * 2. CALLCODE/DELEGATECALL should skip `handle_transfer`, otherwise
   *    `value transfer` of CALLCODE/DELEGATECALL will be executed twice
   */
  if (!is_special_call(msg.kind)) {
    bool to_address_is_eoa = !to_address_exists
                          || (to_address_exists && code_size == 0);
    ret = handle_transfer(ctx, &msg, to_address_is_eoa);
    if (ret != 0) {
      return ret;
    }
  }

  debug_print_int("[handle_message] msg.kind", msg.kind);
  /* NOTE: msg and res are updated */
  if (to_address_exists && code_size > 0) {
    ret = execute_in_evmone(ctx, &msg, parent_from_id, from_id, to_id, code_data, code_size, res);
    if (ret != 0) {
      return ret;
    }
  } else {
    ckb_debug("[handle_message] Don't run evm and return empty data");
    res->output_data = NULL;
    res->output_size = 0;
    res->gas_left = msg.gas;
    res->status_code = EVMC_SUCCESS;
  }

  if (is_create(msg.kind)) {
    /** Store contract code though syscall */
    ret = store_contract_code(ctx, to_id, res);
    if (ret != 0) {
      return ret;
    }

    /**
     * When call kind is CREATE/CREATE2, update create_address of the new
     * contract
     */
    memcpy(res->create_address.bytes, msg.destination.bytes, 20);
  }

  uint32_t used_memory;
  memcpy(&used_memory, res->padding, sizeof(uint32_t));
  debug_print_int("[handle_message] used_memory(Bytes)", used_memory);
  debug_print_int("[handle_message] gas left", res->gas_left);
  debug_print_int("[handle_message] status_code", res->status_code);

  return (int)res->status_code;
}

int emit_evm_result_log(gw_context_t* ctx,
                        const uint64_t gas_used, const int status_code) {
  /*
    data = { gasUsed: u64, cumulativeGasUsed: u64, contractAddress: [u8;20], status_code: u32 }

    data[ 0.. 8] = gas_used
    data[ 8..16] = cumulative_gas_used
    data[16..36] = created_address ([0u8; 20] means not created)
    data[36..40] = status_code (EVM status_code)
   */
  uint64_t cumulative_gas_used = gas_used;
  uint32_t status_code_u32 = (uint32_t)status_code;

  uint32_t data_size = 8 + 8 + 20 + 4;
  uint8_t data[8 + 8 + 20 + 4] = {0};
  uint8_t *ptr = data;
  memcpy(ptr, (uint8_t *)(&gas_used), 8);
  ptr += 8;
  memcpy(ptr, (uint8_t *)(&cumulative_gas_used), 8);
  ptr += 8;
  memcpy(ptr, (uint8_t *)(&g_created_address), 20);
  ptr += 20;
  memcpy(ptr, (uint8_t *)(&status_code_u32), 4);
  ptr += 4;

  /* NOTE: if create account failed the `to_id` will also be `context->to_id` */
  uint32_t to_id = g_created_id == UINT32_MAX ? ctx->transaction_context.to_id : g_created_id;
  /* to_id must already exists here */
  int ret = ctx->sys_log(ctx, to_id, GW_LOG_POLYJUICE_SYSTEM, data_size, data);
  if (ret != 0) {
    debug_print_int("sys_log evm result failed", ret);
    return ret;
  }
  return 0;
}

int clean_evmc_result_and_return(evmc_result *res, int code) {
  if (res->release) res->release(res);
  return code;
}

/**
 * @brief Fill the sender and destination of msg after globals loaded
 */
int fill_msg_sender_and_dest(gw_context_t* ctx, struct evmc_message* msg) {
  gw_transaction_context_t *tx_ctx = &ctx->transaction_context;

  /* Fill msg.sender afert load globals */
  uint8_t from_script_hash[GW_KEY_BYTES] = {0};
  int ret = ctx->sys_get_script_hash_by_account_id(ctx, tx_ctx->from_id,
                                                   from_script_hash);
  if (ret != 0) {
    debug_print_int("get from script hash failed, from_id", tx_ctx->from_id);
    return ret;
  }
  // msg.sender should always be an EOA
  ret = load_eth_address_by_script_hash(ctx, from_script_hash,
                                        msg->sender.bytes);
  if (ret != 0) {
    debug_print_int("load msg->sender failed, from_id", tx_ctx->from_id);
    return ret;
  }
  memcpy(g_tx_origin.bytes, msg->sender.bytes, ETH_ADDRESS_LEN);

  /* Fill msg.destination after load globals */
  if (msg->kind != EVMC_CREATE) {
    uint8_t to_script_hash[GW_KEY_BYTES] = {0};
    ret = ctx->sys_get_script_hash_by_account_id(
        ctx,
        tx_ctx->to_id,
        to_script_hash);
    if (ret != 0) {
      return ret;
    }
    // msg.destination should always be a contract account
    ret = load_eth_address_by_script_hash(ctx, to_script_hash,
                                          msg->destination.bytes);
    if (ret != 0) {
      debug_print_int("load msg.destination failed, to_id",
                      tx_ctx->to_id);
      return ret;
    }
  }

  return 0;
}

int run_polyjuice() {
#ifdef POLYJUICE_DEBUG_LOG
  // init buffer for debug_print
  char buffer[DEBUG_BUFFER_SIZE];
  g_debug_buffer = buffer;

  ckb_debug(POLYJUICE_VERSION);
#endif

  int ret;

  /* prepare context */
  gw_context_t context;
  ret = gw_context_init(&context);
  if (ret != 0) {
    return ret;
  }

  evmc_message msg;
  /* Parse message */
  ckb_debug("BEGIN parse_message()");
  ret = parse_args(&msg, &context);
  ckb_debug("END parse_message()");
  if (ret != 0) {
    return ret;
  }

  /* Ensure the transaction has more gas than the basic tx fee. */
  uint64_t min_gas;
  ret = intrinsic_gas(&msg, is_create(msg.kind), &min_gas);
  if (ret != 0) {
    return ret;
  }
  if ((uint64_t)msg.gas < min_gas) {
    debug_print_int("Insufficient gas limit, should exceed", min_gas);
    return ERROR_INSUFFICIENT_GAS_LIMIT;
  }

  /* Load: validator_code_hash, hash_type, g_sudt_id */
  ret = load_globals(&context, context.transaction_context.to_id);
  if (ret != 0) {
    return ret;
  }

  /**
   * We seperate two branches: 
   * - transfer native token to EOA account
   * - the rest
   *
   * The part of transferring native tokens to EOA account is isolated. It will
   * not enter into EVM. 
   * Recognizing EOA transferring if conditions are satisfied below:
   * - to_id is g_creator_account_id
   * - only accept call_kind == EVMC_CALL
   * - g_eoa_transfer_flag is true
   * - g_eoa_transfer_to_address is not zero address
   * The `g_eoa_transfer_to_address` which is the true `to_address` that is
   * going to transfer to, and must not be a contract address.
   * 
   * Regarding transfer to contract account, a normal polyjuice transaction
   * which `to_id` is the contract account should be expected.
   *
   **/
  if (g_creator_account_id == context.transaction_context.to_id && 
          msg.kind == EVMC_CALL &&
          g_eoa_transfer_flag) {
    ckb_debug("BEGIN handle_native_token_transfer");
    uint256_t value;
    uint8_t* value_ptr = (uint8_t*)&value;
    for (int i = 0; i < 32; i++) {
      value_ptr[i] = msg.value.bytes[31 - i];
    }

    uint64_t gas_used = min_gas;
    gw_reg_addr_t from_addr = {0};
    // handle error later
    int transfer_ret = handle_native_token_transfer(&context, context.transaction_context.from_id,
                                                    value, &from_addr, &gas_used);
    ckb_debug("END handle_native_token_transfer");
    // handle fee
    uint256_t gas_fee = calculate_fee(g_gas_price, gas_used);
    debug_print_int("[handle_native_token_transfer] gas_used", gas_used);
    ret = sudt_pay_fee(&context, g_sudt_id, from_addr, gas_fee);
    // handle native token transfer error
    if (ret != 0) {
      debug_print_int("[handle_native_token_transfer] pay fee to block_producer failed", ret);
      return ret;
    }
    /* emit POLYJUICE_SYSTEM log to Godwoken */
    ret = emit_evm_result_log(&context, gas_used, transfer_ret);
    if (ret != 0) {
      ckb_debug("emit_evm_result_log failed");
      return ret;
    }

    ckb_debug("[handle_native_token_transfer] finalize");
    gw_finalize(&context);
    if (transfer_ret != 0) {
        return transfer_ret;
    }
    return 0;
  }

  ret = fill_msg_sender_and_dest(&context, &msg);
  if (ret != 0) {
    ckb_debug("failed to fill_msg_sender_and_dest");
    return ret;
  }

  uint8_t evm_memory[MAX_EVM_MEMORY_SIZE];
  init_evm_memory(evm_memory, MAX_EVM_MEMORY_SIZE);

  /* init EVM execution result */
  struct evmc_result res;
  memset(&res, 0, sizeof(evmc_result));
  res.status_code = EVMC_FAILURE;      // Generic execution failure
  debug_print_int("[run_polyjuice] initial gas limit", msg.gas);
  int64_t initial_gas = msg.gas;
  msg.gas -= min_gas;                  // subtract IntrinsicGas

  /*
   * Take a snapshot for call/create and revert later if EVM returns an error.
   */
  uint32_t snapshot_id;
  ret = context.sys_snapshot(&context, &snapshot_id);
  debug_print_int("[run_polyjuice] take a snapshot id", snapshot_id);
  if (ret != 0) {
    return ret;
  }
  int ret_handle_message = handle_message(&context, UINT32_MAX, UINT32_MAX,
                                          NULL, &msg, &res);
  // debug_print evmc_result.output_data if the execution failed
  if (res.status_code != 0) {
    /* We must handle revert with snapshot. */
    g_created_id = UINT32_MAX; // revert if new account is created
    memset(g_created_address, 0, 20);
    ret = context.sys_revert(&context, snapshot_id);
    debug_print_int("[run_polyjuice] revert with snapshot id", snapshot_id);
    if (ret != 0) {
      return ret;
    }
    debug_print_int("evmc_result.output_size", res.output_size);
    // The output contains data coming from REVERT opcode
    debug_print_data("evmc_result.output_data:", res.output_data,
                     res.output_size > 100 ? 100 : res.output_size);

    // record the used memory of a failed transaction
    uint32_t used_memory;
    memcpy(&used_memory, res.padding, sizeof(uint32_t));
    debug_print_int("[run_polyjuice] used_memory(Bytes)", used_memory);
  }

  debug_print_int("[run_polyjuice] gas left", res.gas_left);
  uint64_t gas_used =
      (uint64_t)(res.gas_left <= 0 ? initial_gas : initial_gas - res.gas_left);
  debug_print_int("[run_polyjuice] gas_used", gas_used);

  /* emit POLYJUICE_SYSTEM log to Godwoken */
  ret = emit_evm_result_log(&context, gas_used, res.status_code);
  if (ret != 0) {
    ckb_debug("emit_evm_result_log failed");
    return clean_evmc_result_and_return(&res, ret);
  }

  /* Godwoken syscall: SET_RETURN_DATA */
  debug_print_int("set return data size", res.output_size);
  ret = context.sys_set_program_return_data(&context,
                                            (uint8_t *)res.output_data,
                                            res.output_size);
  if (ret != 0) {
    ckb_debug("set return data failed");
    return clean_evmc_result_and_return(&res, ret);
  }

  if (ret_handle_message != 0) {
    ckb_debug("handle message failed");
    return clean_evmc_result_and_return(&res, ret_handle_message);
  }

  /* Handle transaction fee */
  if (res.gas_left < 0) {
    ckb_debug("gas not enough");
    return clean_evmc_result_and_return(&res, -1);
  }
  uint256_t fee_u256 = calculate_fee(g_gas_price, gas_used);
  gw_reg_addr_t sender_addr = new_reg_addr(msg.sender.bytes);
  ret = sudt_pay_fee(&context, g_sudt_id, /* g_sudt_id must already exists */
                     sender_addr, fee_u256);
  if (ret != 0) {
    debug_print_int("[run_polyjuice] pay fee to block_producer failed", ret);
    return clean_evmc_result_and_return(&res, ret);
  }

  /* finalize state */
  ckb_debug("[run_polyjuice] finalize");
  ret = gw_finalize(&context);
  if (ret != 0) {
    return clean_evmc_result_and_return(&res, ret);
  }

  return clean_evmc_result_and_return(&res, 0);
}
