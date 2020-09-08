/*
* The Proxy is a layer2 contract example.
* 
* The Godwoken layer2 contract must be implemented as a shared library.
* We expose two interfaces as the requirement asked:
* - `gw_construct`: contract constructor, be invoked when the contract created
* - `gw_handle_message`: call handler, invoked when a transaction/message send to this contract
*
* This contract takes id and args, then send the args to the id.
*/
#define __SHARED_LIBRARY__ 1

#include "gw_def.h"
#include "godwoken.h"
#include "ckb_syscalls.h"
#include "stdio.h"

#define ERROR_INVALID_DATA 10

/* do nothing */
__attribute__((visibility("default"))) int gw_construct(gw_context_t * ctx) {
    return 0;
}

/* parse args then call another contract */
__attribute__((visibility("default"))) int gw_handle_message(gw_context_t * ctx) {
    if (ctx->call_context.args_len < sizeof(uint32_t)) {
        return ERROR_INVALID_DATA;
    }
    uint32_t id = *(uint32_t *)ctx->call_context.args;
    uint8_t * args = ctx->call_context.args + sizeof(uint32_t);
    uint32_t args_len = ctx->call_context.args_len - sizeof(uint32_t);
    gw_call_receipt_t receipt;
    int ret = ctx->sys_call(ctx, id, args, args_len, &receipt);
    if(ret != 0) { return ret; }
    ctx->sys_set_return_data(ctx, receipt.return_data, receipt.return_data_len);
    return 0;
}
