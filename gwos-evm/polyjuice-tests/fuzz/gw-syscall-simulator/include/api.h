#ifndef GW_SYSCALL_SIMULATOR_API_H_
#define GW_SYSCALL_SIMULATOR_API_H_
#include <cstdint>
#ifdef __cplusplus
extern "C"{
#endif

#include <stddef.h>
#include <stdint.h>

/*=====ckb syscalls====*/
int ckb_exit(int8_t code);
int ckb_debug(const char* s);
/*=====ckb syscalls====*/

/*=====gw syscalls====*/
int gw_load_rollup_config(uint8_t *addr, uint64_t *len);
int gw_store(void *key, void *value);
int gw_load(const uint8_t *key, uint8_t *value);
int gw_set_return_data(const uint8_t *addr, uint64_t len);
int gw_create(const uint8_t *script_addr, uint64_t script_len, uint32_t *account_id);
int gw_load_tx(void *addr, uint64_t *len);
int gw_load_block_info(void *addr, uint64_t *len);
int gw_get_block_hash(uint8_t *addr, uint64_t number);
int gw_store_data(const uint8_t *addr, uint64_t len);
int gw_load_data(void *data, uint64_t *len, uint64_t offset, const uint8_t *data_hash);
int gw_load_account_script(void *script, uint64_t *len, uint64_t offset, uint32_t account_id);
int gw_pay_fee(uint8_t *reg_addr_buf, uint64_t len, uint32_t sudi_id, const uint8_t *amount);
int gw_log(uint32_t account_id, uint8_t service_flag, uint64_t len, const uint8_t *data);
int gw_bn_add(uint8_t *data, uint64_t len, uint64_t offset, const uint8_t *input, uint64_t input_size);
int gw_bn_mul(uint8_t *data, uint64_t len, uint64_t offset, const uint8_t *input, uint64_t input_size);
int gw_bn_pairing(uint8_t *data, uint64_t len, uint64_t offset, const uint8_t *input, uint64_t input_size);
int gw_snapshot(uint32_t *snapshot);
int gw_revert(uint32_t snapshot);
int gw_check_sudt_addr_permission(const uint8_t* sudt_proxy_addr);
/*=====gw syscalls====*/

/*=====utils====*/
int gw_reset();
int gw_set_tx(const uint8_t *addr, uint64_t len);
int gw_create_contract_account(const uint8_t *eth_addr,
                               const uint8_t *mint_addr,
                               const uint8_t *code_addr,
                               uint64_t code_size,
                               uint32_t *account_id);
int gw_create_eoa_account(const uint8_t *eth_addr, const uint8_t *mint_addr, uint32_t *account_id);


#ifdef __cplusplus
}
#endif
#endif
