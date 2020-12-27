/*
 * Meta contract
 * This contract is builtin in the Godwoken Rollup, and the account_id is zero.
 *
 * We use Meta contract to implement some special features like create a
 * contract account.
 */

#include "ckb_syscalls.h"
#include "gw_syscalls.h"
#include "stdio.h"

/* MSG_TYPE */
#define MSG_CREATE_ACCOUNT 0

int main() {
  /* initialize context */
  gw_context_t ctx = {0};
  int ret = gw_context_init(&ctx);
  if (ret != 0) {
    return ret;
  };

  /* parse Meta contract args */
  mol_seg_t args_seg;
  args_seg.ptr = ctx.transaction_context.args;
  args_seg.size = ctx.transaction_context.args_len;
  if (MolReader_MetaContractArgs_verify(&args_seg, false) != MOL_OK) {
    return ERROR_INVALID_DATA;
  }
  mol_union_t msg = MolReader_MetaContractArgs_unpack(&args_seg);

  /* Handle messages */
  if (msg.item_id == MSG_CREATE_ACCOUNT) {
    /* Create account */
    mol_seg_t script_seg = MolReader_CreateAccount_get_script(&msg.seg);
    uint32_t account_id = 0;
    int ret =
        ctx.sys_create(&ctx, script_seg.ptr, script_seg.size, &account_id);
    if (ret != 0) {
      return ret;
    }
    ret = ctx.sys_set_program_return_data(&ctx, (uint8_t *)&account_id,
                                          sizeof(uint32_t));
    if (ret != 0) {
      return ret;
    }
  } else {
    return ERROR_UNKNOWN_MSG;
  }
  return gw_finalize(&ctx);
}
