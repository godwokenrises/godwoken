/* note, this macro should be same as in ckb_syscall.h */
#ifndef CKB_C_STDLIB_CKB_SYSCALLS_H_
#define CKB_C_STDLIB_CKB_SYSCALLS_H_

#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>

#include "ckb_consts.h"
#include "secp256k1_data_info.h"

size_t s_INPUT_SIZE = 0;
uint8_t* s_INPUT_DATA = NULL;

extern "C" int ckb_debug(const char*);
static char debug_buf[64 * 1024];
void dbg_print(const char* fmt, ...) {
    va_list args;
    va_start(args, fmt);
    vsnprintf(debug_buf, sizeof(debug_buf), fmt, args);
    va_end(args);
    ckb_debug(debug_buf);
}
void dbg_print_h256(const uint8_t* h256_ptr) {
  int offset = sprintf(debug_buf, "H256[");
  for (size_t i = 0; i < 31; i++) {
    offset += sprintf(debug_buf + offset, "%d, ", *(h256_ptr + i));
  }
  sprintf(debug_buf + offset, "%d]", *(h256_ptr + 31));
  ckb_debug(debug_buf);
}
void dbg_print_hex(const uint8_t* ptr, size_t size) {
  printf("0x");
  for (size_t i = 0; i < size; i++) {
    printf("%02x", ptr[i]);
  }
  printf("\n");
}

#ifndef POLYJUICE_DEBUG_LOG
#undef dbg_print
#undef dbg_print_h256
#undef dbg_print_hex
#define dbg_print(...) do {} while (0)
#define dbg_print_h256(p) do {} while (0)
#define dbg_print_hex(p) do {} while (0)
#endif

#define MOCK_SUCCESS 0

int ckb_exit(int8_t code) {
  printf("ckb_exit, code=%d\n", code);
  exit(0);
  return MOCK_SUCCESS;
}

// Mock implementation for the SYS_ckb_load_cell_data_as_code syscall in
// _ckb_load_cell_code.
#define syscall(n, a0, a1, a2, a3, a4, a5)                              \
  __internal_syscall(n, (long)(a0), (long)(a1), (long)(a2), (long)(a3), \
                     (long)(a4), (long)(a5))
static int inline __internal_syscall(long n, long _a0, long _a1, long _a2,
                                     long _a3, long _a4, long _a5);

#ifdef GW_GENERATOR
#include "generator_utils.h"
#include "mock_godwoken.hpp"
#endif

static int inline __internal_syscall(long n, long _a0, long _a1, long _a2,
                                     long _a3, long _a4, long _a5) {
  switch (n) {
    // mock syscall(GW_SYS_LOAD_TRANSACTION, addr, &inner_len, 0, 0, 0, 0)
    case GW_SYS_LOAD_TRANSACTION: // Load Layer2 Transaction
      return gw_load_transaction_from_raw_tx((uint8_t *)_a0, (uint64_t *)_a1);

    // mock syscall(GW_SYS_LOAD, raw_key, value, 0, 0, 0, 0)
    case GW_SYS_LOAD:
      dbg_print("====== mock syscall(GW_SYS_LOAD) ======");
      dbg_print("raw_key:");
      dbg_print_h256((uint8_t*)_a0);
      gw_sys_load((uint8_t *)_a0, (uint8_t *)_a1);
      // always return 0, even the key(_a0) is not found
      return MOCK_SUCCESS;

    // mock syscall(GW_SYS_LOAD_DATA, data, &inner_len, offset, data_hash, 0, 0)
    case GW_SYS_LOAD_DATA:
      /* match ckb_secp256k1_data_hash, load secp256k1_data */
      // TODO: move this to fuzz_init() step
      if (0 == memcmp((uint8_t *)_a3, ckb_secp256k1_data_hash, 32)) {
        FILE* stream = fopen("./build/secp256k1_data", "rb");
        int ret = fread((uint8_t *)_a0, CKB_SECP256K1_DATA_SIZE, 1, stream);
        fclose(stream);
        stream = NULL;
        if (ret != 1) { // ret = The total number of elements successfully read
          return GW_ERROR_NOT_FOUND;
        }
        *(uint64_t *)_a1 = CKB_SECP256K1_DATA_SIZE;
        return MOCK_SUCCESS;
      }
      return gw_sys_load_data((uint8_t *)_a0, (uint64_t *)_a1, _a2, (uint8_t *)_a3);

    // mock syscall(GW_SYS_STORE_DATA, data_len, data, 0, 0, 0, 0)
    case GW_SYS_STORE_DATA:
      return gw_store_data(_a0, (uint8_t *)_a1);

    // mock syscall(GW_SYS_SET_RETURN_DATA, *data, len, 0, 0, 0, 0)
    case GW_SYS_SET_RETURN_DATA:
      dbg_print("mock syscall(GW_SYS_SET_RETURN_DATA)");
      gw_sys_set_return_data((uint8_t *)_a0, _a1);
      return MOCK_SUCCESS;

    // mock syscall(GW_SYS_GET_BLOCK_HASH, block_hash, number, 0, 0, 0, 0)
    case GW_SYS_GET_BLOCK_HASH:
      dbg_print("mock syscall(GW_SYS_GET_BLOCK_HASH");
      gw_sys_get_block_hash((uint8_t *)_a0, _a1);
      return MOCK_SUCCESS;

    // mock syscall(GW_SYS_STORE, raw_key, value, 0, 0, 0, 0)
    case GW_SYS_STORE:
      gw_update_raw((uint8_t *)_a0, (uint8_t *)_a1);
      return MOCK_SUCCESS;

    // mock syscall(GW_SYS_LOAD_BLOCKINFO, addr, &inner_len, 0, 0, 0, 0)
    case GW_SYS_LOAD_BLOCKINFO:
      return gw_sys_load_blockinfo((uint8_t *)_a0, (uint64_t *)_a1);

    // mock syscall(GW_SYS_LOAD_ACCOUNT_SCRIPT, script, &inner_len, offset, account_id, 0, 0)
    case GW_SYS_LOAD_ACCOUNT_SCRIPT:
      return gw_sys_load_account_script((uint8_t *)_a0, (uint64_t *)_a1, _a2, _a3);

    // mock syscall(GW_SYS_LOAD_ROLLUP_CONFIG, addr, &inner_len, 0, 0, 0, 0)
    case GW_SYS_LOAD_ROLLUP_CONFIG:
      return gw_sys_load_rollup_config((uint8_t *)_a0, (uint64_t *)_a1);

    // mock syscall(GW_SYS_CREATE, script, script_len, account_id, 0, 0, 0)
    case GW_SYS_CREATE:
      return gw_sys_create((uint8_t *)_a0, _a1, (uint32_t *)_a2);

    // mock syscall(GW_SYS_LOG, account_id, service_flag, data_length, data, 0, 0)
    case GW_SYS_LOG: // TODO: @see emit_evm_result_log
      dbg_print("[GW_SYS_LOG] service_flag[%d] account[%d] ", (uint8_t)_a1, _a1);
      return 0;

    // mock syscall(GW_SYS_PAY_FEE, payer_addr, short_addr_len, sudt_id, &amount, 0, 0)
    case GW_SYS_PAY_FEE:
      // TODO: payer: payer_addr[short_addr_len]
      dbg_print("[mock SYS_PAY_FEE] sudt_id: %d, amount: %ld",
                (uint32_t)_a2, *(uint128_t *)_a3);
      return 0;

    default:
      return GW_ERROR_NOT_FOUND;
  }
}
#endif
