#ifndef GW_ETH_ADDR_REG_H
#define GW_ETH_ADDR_REG_H

#include "ckb_syscalls.h"
#include "gw_def.h"
#include "gw_syscalls.h"

#define GW_ETH_ADDRESS_LEN 20
#define GW_CONTRACT_ACCOUNT_SCRIPT_ARGS_LEN 56 /* 32 + 4 + 20 */
#define GW_CREATOR_SCRIPT_ARGS_LEN 40          /* 32 + 4 + 4  */

/**
 * @brief register a created account into `ETH Address Registry`
 *
 * @param ctx gw_context
 * @param eth_address there are two ETH account types:
 * 1. Externally-owned – controlled by anyone with the private keys
 * 2. Contract – a smart contract deployed to the network, controlled by code
 * @param script_hash Godwoken account script hash
 * @param overwrite re-map if the account has been registered
 * @return int: 0 means success
 */
int gw_update_eth_address_register(
    gw_context_t *ctx, const uint8_t eth_address[GW_ETH_ADDRESS_LEN],
    const uint8_t script_hash[GW_VALUE_BYTES], bool overwrite) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  if (_is_zero_hash((uint8_t *)script_hash)) {
    printf("gw_update_eth_address_register script hash is zero");
    return GW_FATAL_INVALID_DATA;
  }

  gw_reg_addr_t addr = {0};
  addr.reg_id = GW_DEFAULT_ETH_REGISTRY_ACCOUNT_ID;
  addr.addr_len = GW_ETH_ADDRESS_LEN;
  memcpy(addr.addr, eth_address, GW_ETH_ADDRESS_LEN);

  /* check if the account has been registered */
  uint8_t _buf[32] = {0};
  int ret = ctx->sys_get_script_hash_by_registry_address(ctx, &addr, _buf);
  if (ret == 0 && !overwrite) {
    return GW_REGISTRY_ERROR_DUPLICATE_MAPPING;
  }

  /* clear old mapping */
  if (ret == 0 && overwrite) {
    uint8_t script_hash_to_eth_key[36] = {0};
    _gw_build_script_hash_to_registry_address_key(script_hash_to_eth_key,
                                                  (uint8_t *)_buf);
    uint8_t zero_value[32] = {0};
    int ret = ctx->sys_store(ctx, GW_DEFAULT_ETH_REGISTRY_ACCOUNT_ID,
                             script_hash_to_eth_key, 36, zero_value);
    if (ret != 0) {
      return ret;
    }
  }

  /* eth_address -> gw_script_hash */
  uint8_t eth_to_script_hash_key[32] = {0};
  ret = _gw_build_registry_address_to_script_hash_key(eth_to_script_hash_key,
                                                      &addr);
  if (ret != 0) {
    return ret;
  }
  ret = ctx->sys_store(ctx, GW_DEFAULT_ETH_REGISTRY_ACCOUNT_ID,
                       eth_to_script_hash_key, 32, script_hash);
  if (ret != 0) {
    return ret;
  }

  /* gw_script_hash -> eth_address */
  uint8_t script_hash_to_eth_key[36] = {0};
  _gw_build_script_hash_to_registry_address_key(script_hash_to_eth_key,
                                                (uint8_t *)script_hash);
  uint8_t addr_buf[32] = {0};
  _gw_cpy_addr(addr_buf, addr);
  ret = ctx->sys_store(ctx, GW_DEFAULT_ETH_REGISTRY_ACCOUNT_ID,
                       script_hash_to_eth_key, 36, addr_buf);
  if (ret != 0) {
    return ret;
  }

  return 0;
}

/**
 * @brief register an account into `ETH Address Registry` by its script_hash
 *
 * Option 1: ETH EOA (externally owned account)
 * Option 2: Polyjuice Contract Account
 *
 * @param ctx gw_context
 * @param script_hash this account should be created on Godwoken
 * @return int: 0 means success
 *
 * NOTICE: We should avoid address conflict between EOA and contract.
 *
 * Ethereum addresses are currently only 160 bits long. This means it is
 * possible to create a collision between a contract account and an Externally
 * Owned Account (EOA) using an estimated 2**80 computing operations, which is
 * feasible now given a large budget (ca. 10 billion USD).
 *
 * See https://eips.ethereum.org/EIPS/eip-3607
 */
int gw_register_eth_address(gw_context_t *ctx,
                            uint8_t script_hash[GW_VALUE_BYTES]) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  int ret;

  // check account existence
  uint32_t account_id;
  ret = ctx->sys_get_account_id_by_script_hash(ctx, script_hash, &account_id);
  if (ret != 0) {
    return GW_ERROR_ACCOUNT_NOT_EXISTS;
  }

  // get the script of the account
  uint8_t script_buffer[GW_MAX_SCRIPT_SIZE];
  uint64_t script_len = GW_MAX_SCRIPT_SIZE;
  ret = ctx->sys_get_account_script(ctx, account_id, &script_len, 0,
                                    script_buffer);
  if (ret != 0) {
    return ret;
  }
  mol_seg_t script_seg;
  script_seg.ptr = script_buffer;
  script_seg.size = script_len;
  if (MolReader_Script_verify(&script_seg, false) != MOL_OK) {
    return GW_ERROR_INVALID_ACCOUNT_SCRIPT;
  }
  mol_seg_t script_code_hash_seg = MolReader_Script_get_code_hash(&script_seg);

  // get rollup_config to compare with
  mol_seg_t rollup_config_seg;
  rollup_config_seg.ptr = ctx->rollup_config;
  rollup_config_seg.size = ctx->rollup_config_size;

  mol_seg_t args_seg = MolReader_Script_get_args(&script_seg);
  mol_seg_t raw_bytes_seg = MolReader_Bytes_raw_bytes(&args_seg);
  uint8_t eth_address[GW_ETH_ADDRESS_LEN] = {0};

  /**
   * Option 1: ETH EOA (externally owned account) account
   */
  mol_seg_t allowed_eoa_list_seg =
      MolReader_RollupConfig_get_allowed_eoa_type_hashes(&rollup_config_seg);
  uint32_t len = MolReader_AllowedTypeHashVec_length(&allowed_eoa_list_seg);
  for (uint32_t i = 0; i < len; i++) {
    mol_seg_res_t allowed_type_hash_res =
        MolReader_AllowedTypeHashVec_get(&allowed_eoa_list_seg, i);

    if (allowed_type_hash_res.errno != MOL_OK) {
      return GW_FATAL_INVALID_DATA;
    }

    mol_seg_t type_seg =
        MolReader_AllowedTypeHash_get_type_(&allowed_type_hash_res.seg);
    if (*(uint8_t *)type_seg.ptr == GW_ALLOWED_EOA_ETH) {
      mol_seg_t eth_lock_code_hash_seg =
          MolReader_AllowedTypeHash_get_hash(&allowed_type_hash_res.seg);

      if (memcmp(script_code_hash_seg.ptr, eth_lock_code_hash_seg.ptr,
                 script_code_hash_seg.size) == 0) {
        ckb_debug(
            "[gw_register_eth_address] This is an ETH externally owned "
            "account");
        if (raw_bytes_seg.size != 52) {
          ckb_debug("[gw_register_eth_address] not eth_account_lock");
          return GW_FATAL_UNKNOWN_ARGS;
        }
        _gw_fast_memcpy(eth_address, raw_bytes_seg.ptr + 32,
                        GW_ETH_ADDRESS_LEN);
        return gw_update_eth_address_register(ctx, eth_address, script_hash,
                                              false);
      }
    }
  }

  /**
   * Option 2: Polyjuice Contract Account
   *
   * There are 2 major ways in which a Polyjuice smart contract can be deployed:
   *
   * 1. CREATE Flow:
   *   The address of an normal contract is deterministically computed from
   * the address of its creator (sender) and how many transactions the creator
   * has sent (nonce). The sender and nonce are RLP encoded and then hashed with
   * Keccak-256.
   *   `eth_address = hash(sender, nonce)`
   *
   * 2. CREATE2 Flow (EIP-1014):
   *   This is a way to say: “I'll deploy this contract at this address in the
   * future."
   *   `eth_address = hash(0xFF, sender, salt, bytecode)`
   *
   * See {create_new_account} in polyjuice.h
   */
  mol_seg_t allowed_contract_list_seg =
      MolReader_RollupConfig_get_allowed_contract_type_hashes(
          &rollup_config_seg);
  len = MolReader_AllowedTypeHashVec_length(&allowed_contract_list_seg);
  for (uint32_t i = 0; i < len; i++) {
    mol_seg_res_t allowed_type_hash_res =
        MolReader_AllowedTypeHashVec_get(&allowed_eoa_list_seg, i);

    if (allowed_type_hash_res.errno != MOL_OK) {
      ckb_debug("[gw_register_eth_address] failed to get Polyjuice code_hash");
      return GW_FATAL_INVALID_DATA;
    }

    mol_seg_t type_seg =
        MolReader_AllowedTypeHash_get_type_(&allowed_type_hash_res.seg);
    if (*(uint8_t *)type_seg.ptr == GW_ALLOWED_CONTRACT_POLYJUICE) {
      mol_seg_t polyjuice_code_hash_seg =
          MolReader_AllowedTypeHash_get_hash(&allowed_type_hash_res.seg);

      if (memcmp(script_code_hash_seg.ptr, polyjuice_code_hash_seg.ptr,
                 script_code_hash_seg.size) == 0) {
        ckb_debug(
            "[gw_register_eth_address] This is a Polyjuice contract account");
        if (raw_bytes_seg.size != GW_CONTRACT_ACCOUNT_SCRIPT_ARGS_LEN) {
          ckb_debug(
              "[gw_register_eth_address] not Polyjuice contract script_args");
          return GW_FATAL_UNKNOWN_ARGS;
        }
        _gw_fast_memcpy(eth_address, raw_bytes_seg.ptr + 36,
                        GW_ETH_ADDRESS_LEN);
        return gw_update_eth_address_register(ctx, eth_address, script_hash,
                                              false);
      }
    }
  }

  return GW_ERROR_UNKNOWN_SCRIPT_CODE_HASH;
}

#endif
