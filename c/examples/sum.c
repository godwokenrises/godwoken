/*
* The Sum is a layer2 contract example.
* 
* The Godwoken layer2 contract must be implemented as a shared library.
* We expose two interfaces as the requirement asked:
* - `gw_construct`: contract constructor, be invoked when the contract created
* - `gw_handle_message`: call handler, invoked when a transaction/message send to this contract
*/
#define __SHARED_LIBRARY__ 1

#include "gw_def.h"
#include "godwoken.h"

#define ERROR_INVALID_DATA 10


uint64_t saturating_add(uint64_t a, uint64_t b);
int extract_args(gw_context_t * ctx, uint64_t * v);
int read_counter(gw_context_t * ctx, uint64_t * counter);
int write_counter(gw_context_t * ctx, uint64_t counter);

/* transtions's args should be a uint64_t as the initial value */
__attribute__((visibility("default"))) int gw_construct(gw_context_t * ctx) {
    uint64_t init_value;
    int ret = extract_args(ctx, &init_value);
    if(ret != 0) { return ret; }
    /* return current counter value as data */
    ctx->sys_set_return_data(ctx, (uint8_t *)&init_value, sizeof(uint64_t));
    return write_counter(ctx, init_value);
}

/* transtions's args should be a uint64_t as the accumulate number */
__attribute__((visibility("default"))) int gw_handle_message(gw_context_t * ctx) {
    uint64_t counter_value;
    int ret = read_counter(ctx, &counter_value);
    if(ret != 0) { return ret; }
    uint64_t add_value;
    ret = extract_args(ctx, &add_value);
    if(ret != 0) { return ret; }
    counter_value = saturating_add(counter_value, add_value);
    /* return current counter value as data */
    ctx->sys_set_return_data(ctx, (uint8_t *)&counter_value, sizeof(uint64_t));
    return write_counter(ctx, counter_value);
}

/* helper functions */

uint64_t saturating_add(uint64_t a, uint64_t b)
{
  uint64_t c = a + b;
  if (c<a) { c = -1;}
  return c;
}

int extract_args(gw_context_t * ctx, uint64_t * v) {
    mol_seg_t call_context_seg;
    call_context_seg.ptr = ctx->call_context;
    call_context_seg.size = ctx->call_context_len;
    mol_seg_t args_seg = MolReader_CallContext_get_args(&call_context_seg);
    mol_seg_t raw_bytes_seg =
      MolReader_Bytes_raw_bytes(&args_seg);
    if(sizeof(uint64_t) != raw_bytes_seg.size) {
        return ERROR_INVALID_DATA;
    }
    *v = *(uint64_t *)raw_bytes_seg.ptr;
    return 0;
}

int read_counter(gw_context_t * ctx, uint64_t * counter) {
    uint8_t key[GW_KEY_BYTES];
    ctx->blake2b_hash(key, (uint8_t *)"counter", 7);
    uint8_t value[GW_VALUE_BYTES];
    int ret = ctx->sys_load(ctx, key, value);
    if( ret != 0 ) { return ret; }
    *counter = *(uint64_t *)value;
    return 0;
}

int write_counter(gw_context_t * ctx, uint64_t counter) {
    uint8_t key[GW_KEY_BYTES];
    ctx->blake2b_hash(key, (uint8_t *)"counter", 7);
    uint8_t value[GW_VALUE_BYTES];
    *(uint64_t *)value = counter;
    return ctx->sys_store(ctx, key, value);
}