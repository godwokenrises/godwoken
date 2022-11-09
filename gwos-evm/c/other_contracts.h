
#ifndef OTHER_CONTRACTS_H_
#define OTHER_CONTRACTS_H_

#include "polyjuice_utils.h"

/* Gas fee */
#define RECOVER_ACCOUNT_GAS                    3600 /* more than ecrecover */

int recover_account_gas(const uint8_t* input_src,
                        const size_t input_size,
                        uint64_t* gas) {
  *gas = RECOVER_ACCOUNT_GAS;
  return 0;
}

/* Recover an EoA account script hash by signature

  input: (the input data is from abi.encode(mesage, signature, code_hash))
  ======
    input[ 0..32]  => message
    input[32..64]  => offset of signature part
    input[64..96]  => code_hash (EoA lock hash)
    input[96..128] => length of signature data
    input[128..]   => signature data

  output (32 bytes):
  =======
    output[0..32] => account script hash
 */
int recover_account(gw_context_t* ctx,
                    const uint8_t* code_data,
                    const size_t code_size,
                    const enum evmc_call_kind parent_kind,
                    bool is_static_call,
                    const uint8_t* input_src,
                    const size_t input_size,
                    uint8_t** output, size_t* output_size) {
  if (input_size < 128) {
    debug_print_int("input size too small", input_size);
    return ERROR_RECOVER_ACCOUNT;
  }
  int ret;
  uint8_t *message = (uint8_t *)input_src;
  uint8_t *code_hash = (uint8_t *)input_src + 64;
  uint8_t *signature = (uint8_t *)input_src + 128;
  uint64_t signature_len = 0;
  ret = parse_u64(input_src + 96, &signature_len);
  if (ret != 0) {
    debug_print_int("parse signature length failed", ret);
    return ERROR_RECOVER_ACCOUNT;
  }
  if (signature_len + 128 > input_size) {
    debug_print_int("invalid input_size", input_size);
    return ERROR_RECOVER_ACCOUNT;
  }
  uint8_t script[GW_MAX_SCRIPT_SIZE];
  uint64_t script_len = GW_MAX_SCRIPT_SIZE;
  ret = ctx->sys_recover_account(ctx, message, signature, signature_len, code_hash, script, &script_len);
  if (ret != 0) {
    debug_print_int("call sys_recover_account failed", ret);
    /* wrong code_hash is fatal, so we return the error code here */
    if (is_fatal_error(ret)) {
      return FATAL_PRECOMPILED_CONTRACTS;
    } else {
      return ERROR_RECOVER_ACCOUNT;
    }
  }
  *output = (uint8_t *)malloc(32);
  if (*output == NULL) {
    ckb_debug("malloc failed");
    return FATAL_PRECOMPILED_CONTRACTS;
  }
  *output_size = 32;
  blake2b_hash(*output, script, script_len);
  return 0;
}

#endif  /* #define OTHER_CONTRACTS_H_ */
