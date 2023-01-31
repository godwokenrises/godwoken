/* note, this macro should be same as in ckb_syscall.h */
#ifndef CKB_C_STDLIB_CKB_SYSCALLS_H_
#define CKB_C_STDLIB_CKB_SYSCALLS_H_

#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>

#include "api.h"

/* syscalls */
/* Syscall account store / load / create */
#define GW_SYS_CREATE 3100
#define GW_SYS_STORE 3101
#define GW_SYS_LOAD 3102
#define GW_SYS_LOAD_ACCOUNT_SCRIPT 3105
/* Syscall call / return */
#define GW_SYS_SET_RETURN_DATA 3201
/* Syscall data store / load */
#define GW_SYS_STORE_DATA 3301
#define GW_SYS_LOAD_DATA 3302
/* Syscall load metadata structures */
#define GW_SYS_LOAD_ROLLUP_CONFIG 3401
#define GW_SYS_LOAD_TRANSACTION 3402
#define GW_SYS_LOAD_BLOCKINFO 3403
#define GW_SYS_GET_BLOCK_HASH 3404
/* Syscall builtins */
#define GW_SYS_PAY_FEE 3501
#define GW_SYS_LOG 3502
#define GW_SYS_RECOVER_ACCOUNT 3503
/* Syscall for make use the Barreto-Naehrig (BN) curve construction */
#define GW_SYS_BN_ADD 3601
#define GW_SYS_BN_MUL 3602
#define GW_SYS_BN_PAIRING 3603
/* Syscall state */
#define GW_SYS_SNAPSHOT 3701
#define GW_SYS_REVERT 3702
/* Syscall permissions */
#define GW_SYS_CHECK_SUDT_ADDR_PERMISSION 3801

#define GW_ERROR_NOT_FOUND 83

// Mock implementation for the SYS_ckb_load_cell_data_as_code syscall in
// _ckb_load_cell_code.
#define syscall(n, a0, a1, a2, a3, a4, a5)                              \
  __internal_syscall(n, (long)(a0), (long)(a1), (long)(a2), (long)(a3), \
      (long)(a4), (long)(a5))
static int inline __internal_syscall(long n, long _a0, long _a1, long _a2,
    long _a3, long _a4, long _a5);

#ifdef GW_GENERATOR
#include "generator_utils.h"
#endif

static int inline __internal_syscall(long n, long a0, long a1, long a2,
    long a3, long a4, long a5) {
  switch (n) {
    case GW_SYS_CREATE:
      return gw_create((uint8_t*)a0, (uint64_t)a1, (uint32_t*)a2);
    case GW_SYS_STORE:
      return gw_store((uint8_t*)a0, (uint8_t*)a1);
    case GW_SYS_LOAD:
      return gw_load((uint8_t*)a0, (uint8_t*)a1);
    case GW_SYS_LOAD_ACCOUNT_SCRIPT:
      return gw_load_account_script((void*)a0, (uint64_t*)a1, (uint64_t)a2, (uint32_t)a3);
    case GW_SYS_SET_RETURN_DATA:
      return gw_set_return_data((uint8_t*)a0, (uint64_t)a1);
    case GW_SYS_STORE_DATA:
      return gw_store_data((uint8_t*)a0, (uint64_t)a1);
    case GW_SYS_LOAD_DATA:
      return gw_load_data((void*)a0, (uint64_t*)a1, (uint64_t)a2, (uint8_t*)a3);
    case GW_SYS_LOAD_ROLLUP_CONFIG:
      return gw_load_rollup_config((uint8_t*)a0, (uint64_t*)a1);
    case GW_SYS_LOAD_TRANSACTION:
      return gw_load_tx((void*)a0, (uint64_t*)a1);
    case GW_SYS_LOAD_BLOCKINFO:
      return gw_load_block_info((void*)a0, (uint64_t*)a1);
    case GW_SYS_GET_BLOCK_HASH:
      return gw_get_block_hash((uint8_t*)a0, (uint64_t)a1);
    case GW_SYS_PAY_FEE:
      return 0;
    case GW_SYS_LOG:
      return gw_log((uint32_t)a0, (uint8_t)a1, (uint64_t)a2, (uint8_t*)a3);
    case GW_SYS_BN_ADD:
      return gw_bn_add((uint8_t*)a0, (uint64_t)a1, (uint64_t)a2, (uint8_t*)a3, (uint64_t)a4);
    case GW_SYS_BN_MUL:
      return gw_bn_mul((uint8_t*)a0, (uint64_t)a1, (uint64_t)a2, (uint8_t*)a3, (uint64_t)a4);
    case GW_SYS_BN_PAIRING:
      return gw_bn_pairing((uint8_t*)a0, (uint64_t)a1, (uint64_t)a2, (uint8_t*)a3, (uint64_t)a4);
    case GW_SYS_SNAPSHOT:
      return gw_snapshot((uint32_t*)a0);
    case GW_SYS_REVERT:
      return gw_revert((uint32_t)a0);
    case GW_SYS_CHECK_SUDT_ADDR_PERMISSION:
      return gw_check_sudt_addr_permission((uint8_t*)a0);
    case GW_SYS_RECOVER_ACCOUNT:
      return 0;
    default:
      return GW_ERROR_NOT_FOUND;
  }
}
#endif
