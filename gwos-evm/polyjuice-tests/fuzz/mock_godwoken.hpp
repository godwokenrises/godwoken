#include <iostream>

#include <evmc/evmc.hpp>
#include <evmc/mocked_host.hpp>
#include <evmc/hex.hpp>

#include <polyjuice_globals.h>

using namespace std;
using namespace evmc;

class MockedGodwoken : public MockedHost {
public:
  uint32_t account_count = 0;
  unordered_map<bytes32, bytes32> state;
  unordered_map<bytes32, bytes> code_store;
  uint8_t rollup_config[GW_MAX_ROLLUP_CONFIG_SIZE];
  uint32_t rollup_config_size;

  result call(const evmc_message& msg) noexcept override {
    auto result = MockedHost::call(msg);
    return result;
  }

  /// Overwrite get_block_hash (EVMC host method).
  /// disable blockhashes record
  bytes32 get_block_hash(int64_t block_number) const noexcept override {
    return block_hash;
  }
};

struct fuzz_input {
  evmc_revision rev{};
  evmc_message msg{};
  bytes raw_tx;
  MockedGodwoken mock_gw;

  /// Creates invalid input.
  fuzz_input() noexcept { msg.gas = -1; }

  explicit operator bool() const noexcept { return msg.gas != -1; }
};
auto in = fuzz_input{};
MockedGodwoken* gw_host = &in.mock_gw;


extern "C" int ckb_debug(const char* str) {
  cout << "[debug] " << str << endl;
  return 0;
}

#ifndef POLYJUICE_DEBUG_LOG
#undef ckb_debug
#define ckb_debug(s) do {} while (0)
#endif

inline ostream& operator<<(ostream& stream, const bytes32& b32) {
  stream << "H256[";
  for (size_t i = 0; i < 31; i++)
    stream << (uint16_t)b32.bytes[i] << ", ";
  return stream << (uint16_t)b32.bytes[31] << ']';
}
inline ostream& operator<<(ostream& stream, const bytes& bs) {
  for (auto &&i : bs){
    stream << (uint16_t)i << ' ';
  }
  return stream;
}

bytes32 u256_to_bytes32(const uint8_t u8[32]) {
  auto ret = bytes32{};
  memcpy(ret.bytes, u8, 32);
  return ret;
}

// void dbg_print_bytes32(bytes32& b32) {
//   dbg_print(<< b32);
// }

extern "C" void gw_update_raw(const uint8_t k[GW_KEY_BYTES], const uint8_t v[GW_KEY_BYTES]){
  in.mock_gw.state[u256_to_bytes32(k)] = u256_to_bytes32(v);
}

/* store code or script */
extern "C" int gw_store_data(const uint64_t len, uint8_t *data) {
  uint8_t script_hash[GW_KEY_BYTES];
  blake2b_hash(script_hash, data, len);


  if (len < 0 || len > GW_MAX_DATA_SIZE) {
    dbg_print("[gw_store_data] !!!!!! warning: data_len = %ld !!!!!!", len);
    dbg_print("Exceeded max store data size");
    return GW_FATAL_INVALID_DATA;
  }

  // store(data_hash_key, H256::one) insert data hash into SMT
  uint8_t data_hash_key[GW_KEY_BYTES];
  gw_build_data_hash_key(script_hash, data_hash_key);
  uint32_t one = 1;
  uint8_t h256_one[GW_VALUE_BYTES] = {0};
  memcpy(h256_one, &one, sizeof(uint32_t));
  gw_update_raw(data_hash_key, h256_one);

  gw_host->code_store[u256_to_bytes32(script_hash)] = bytes((uint8_t *)data, len);
  return 0;
}

extern "C" int gw_sys_load_data(uint8_t *addr,
                                uint64_t *len_ptr,
                                uint64_t offset,
                                uint8_t data_hash[GW_KEY_BYTES]) {
  auto search = gw_host->code_store.find(u256_to_bytes32(data_hash));
  if (search == gw_host->code_store.end()) {
    return GW_ERROR_NOT_FOUND;
  }
  *len_ptr = search->second.size();
  search->second.copy(addr, *len_ptr);
  return 0;
}

void print_state() {
  for (auto kv : gw_host->state) {
    cout << "\t key:\t" << kv.first << endl << "\t value:\t" << kv.second << endl;
  }
}

// sys_load from state
extern "C" int gw_sys_load(const uint8_t k[GW_KEY_BYTES], uint8_t v[GW_KEY_BYTES]) {
  auto search = gw_host->state.find(u256_to_bytes32(k));
  if (search == gw_host->state.end()) {
    dbg_print("gw_sys_load failed, missing key:");
    dbg_print_h256(k);
    // dbg_print("all the state as following:");
    // print_state();
    return GW_ERROR_NOT_FOUND;
  }
  memcpy(v, search->second.bytes, GW_KEY_BYTES);
  return 0;
}

// load raw layer2 transaction data from fuzzInput.raw_tx
extern "C" int gw_load_transaction_from_raw_tx(uint8_t* addr, uint64_t* len) {
  *len = in.raw_tx.size();
  in.raw_tx.copy(addr, *len);
  return 0;
}

extern "C" void gw_sys_set_return_data(uint8_t* addr, uint64_t len) {
  // should not make a new result
  // in.mock_gw.call_result = make_result(evmc_status_code{}, 0, addr, len);
  dbg_print("gw_sys_set_return_data:");
  // cout << bytes(addr, len) << endl;
}

extern "C" void gw_sys_get_block_hash(uint8_t block_hash[GW_KEY_BYTES], uint64_t number) {
  memcpy(block_hash, gw_host->get_block_hash(number).bytes, GW_KEY_BYTES);
}

extern "C" int gw_sys_load_blockinfo(uint8_t* bi_addr, uint64_t* len_ptr) {
  /** 
   * TODO: block_info fuzzInput
   * struct BlockInfo {
   *  block_producer_id: Uint32,
   *  number: Uint64,
   *  timestamp: Uint64}
   */
  const uint8_t mock_new_block_info[] = {0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0};
  *len_ptr = sizeof(mock_new_block_info);
  dbg_print("mock_new_block_info to "
            "{0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0}");
  memcpy((uint8_t*)bi_addr, mock_new_block_info, *len_ptr);
  return 0;
}

extern "C" int gw_sys_load_script_hash_by_account_id(const uint32_t account_id, uint8_t script_hash[GW_KEY_BYTES]) {
  // dbg_print("sys_get_script_hash_by_account_id account_id = %d", account_id);

  uint8_t key[32] = {0};
  gw_build_account_field_key(account_id, GW_ACCOUNT_SCRIPT_HASH, key);
  return gw_sys_load(key, script_hash);

  // FIXME read script_hash from mock State+CodeStore
  // static const uint8_t test_script_hash[6][32] = {
  //   {231, 196, 69, 164, 212, 229, 83, 6, 137, 240, 237, 105, 234, 223, 101, 133, 197, 66, 85, 214, 112, 85, 87, 71, 17, 170, 138, 126, 128, 173, 186, 76},
  //   {50, 15, 9, 23, 166, 82, 42, 69, 226, 148, 203, 184, 168, 8, 210, 62, 226, 187, 187, 21, 122, 141, 152, 55, 88, 230, 63, 204, 23, 3, 166, 102},
  //   {221, 60, 233, 16, 227, 19, 49, 118, 137, 43, 193, 160, 145, 21, 141, 6, 43, 206, 191, 210, 105, 160, 112, 23, 155, 184, 101, 113, 47, 247, 216, 122},
  //   {48, 160, 141, 250, 92, 214, 34, 124, 231, 78, 106, 179, 173, 80, 61, 55, 161, 156, 45, 114, 214, 222, 9, 77, 4, 104, 52, 44, 30, 149, 27, 36},
  //   {103, 167, 175, 25, 71, 242, 5, 31, 102, 236, 38, 188, 223, 212, 241, 99, 13, 4, 40, 150, 151, 55, 40, 147, 64, 29, 108, 50, 37, 159, 55, 137},
  //   {125, 181, 86, 185, 69, 172, 188, 175, 36, 25, 118, 119, 114, 72, 199, 183, 204, 25, 147, 120, 109, 220, 192, 171, 10, 235, 47, 230, 42, 210, 169, 223}};
}

// extern "C" int gw_sys_get_script_hash_by_short_address(uint8_t *script_hash_addr,
//                                                        uint8_t *prefix_addr,
//                                                        uint64_t prefix_len) {
//   for (auto pair : gw_host->code_store) {
//     if (0 == memcmp(pair.first.bytes, prefix_addr, prefix_len)) {
//       memcpy(script_hash_addr, pair.first.bytes, sizeof(pair.first.bytes));
//       return 0;
//     }
//   }
  
//   dbg_print("gw_sys_get_script_hash_by_short_address failed");
//   return GW_ERROR_NOT_FOUND;
// }

extern "C" int gw_sys_load_account_id_by_script_hash(uint8_t *script_hash,
                                                     uint32_t *account_id_ptr) {
  uint8_t raw_id_key[GW_KEY_BYTES];
  gw_build_script_hash_to_account_id_key(script_hash, raw_id_key);
  uint8_t result_addr[GW_KEY_BYTES];
  int ret = gw_sys_load(raw_id_key, result_addr);
  if (ret != 0) return ret;
  *account_id_ptr = *((uint32_t *)result_addr);
  return 0;
}

extern "C" int gw_sys_load_account_script(uint8_t *script_addr,
                                          uint64_t *len_ptr,
                                          const uint64_t offset,
                                          const uint32_t account_id) {
  uint8_t script_hash[GW_KEY_BYTES];
  int ret = gw_sys_load_script_hash_by_account_id(account_id, script_hash);
  if (ret != 0) {
    return ret;
  }
  return gw_sys_load_data(script_addr, len_ptr, offset, script_hash);
}

extern "C" int gw_sys_load_rollup_config(uint8_t *addr,
                                         uint64_t *len_ptr) {
  *len_ptr = gw_host->rollup_config_size;
  memcpy(addr, gw_host->rollup_config, *len_ptr);
  return 0;
}

/// mock_mint_sudt on layer2
void mock_mint_sudt(uint32_t sudt_id, uint32_t account_id, uint128_t balance)
{
  uint8_t script_hash[GW_KEY_BYTES] = {0};
  gw_sys_load_script_hash_by_account_id(account_id, script_hash);

  // _sudt_build_key
  uint8_t key[GW_KEY_BYTES + 8] = {0};
  uint64_t key_len = DEFAULT_SHORT_SCRIPT_HASH_LEN + 8;
  const uint32_t SUDT_KEY_FLAG_BALANCE = 1;
  const uint32_t short_script_hash_len = DEFAULT_SHORT_SCRIPT_HASH_LEN;
  memcpy(key, (uint8_t *)(&SUDT_KEY_FLAG_BALANCE), sizeof(uint32_t));
  memcpy(key + 4, (uint8_t *)(&short_script_hash_len), sizeof(uint32_t));
  memcpy(key + 8, script_hash, short_script_hash_len);
  uint8_t raw_key[GW_KEY_BYTES];
  gw_build_account_key(sudt_id, key, key_len, raw_key);

  uint8_t value[32] = {0};
  *(uint128_t *)value = balance;

  // sys_store balance
  gw_update_raw(raw_key, value);
}

extern "C" int gw_sys_create(uint8_t *script, uint64_t script_len, uint32_t *account_id_ptr) {
  int ret = 0;

  // Return error if script_hash is exists
  uint8_t script_hash[GW_KEY_BYTES];
  blake2b_hash(script_hash, script, script_len);
  if (0 == gw_sys_load_account_id_by_script_hash(script_hash, account_id_ptr)) {
    dbg_print("script_hash is exists");
    return GW_ERROR_DUPLICATED_SCRIPT_HASH;
  }

  // check script hash type
  mol_seg_t script_seg;
  script_seg.ptr = script;
  script_seg.size = script_len;
  mol_seg_t hash_type_seg = MolReader_Script_get_hash_type(&script_seg);
  const uint8_t SCRIPT_HASH_TYPE_TYPE = 1;
  if ((*(uint8_t *)hash_type_seg.ptr) != SCRIPT_HASH_TYPE_TYPE) {
    dbg_print("script hash type = %d", *(uint8_t *)hash_type_seg.ptr);
    return GW_ERROR_UNKNOWN_SCRIPT_CODE_HASH;
  }

#pragma push_macro("errno")
#undef errno
  // Check script validity
  mol_seg_t code_hash_seg = MolReader_Script_get_code_hash(&script_seg);
  if (code_hash_seg.size != 32) {
    dbg_print("[GW_FATAL_INVALID_DATA] MolReader_Script_get_code_hash");
    return GW_FATAL_INVALID_DATA;
  }
  /* check allowed EOA list */
  mol_seg_t rollup_config_seg = {gw_host->rollup_config, gw_host->rollup_config_size};
  mol_seg_t eoa_list_seg =
    MolReader_RollupConfig_get_allowed_eoa_type_hashes(&rollup_config_seg);
  uint32_t len = MolReader_Byte32Vec_length(&eoa_list_seg);
  bool is_eos_account = false;
  for (uint32_t i = 0; i < len; i++) {
    mol_seg_res_t allowed_code_hash_res = MolReader_Byte32Vec_get(&eoa_list_seg, i);
    if (memcmp(allowed_code_hash_res.seg.ptr, hash_type_seg.ptr, code_hash_seg.size) != 0) {
      continue;
    }
    if (allowed_code_hash_res.errno != MOL_OK ||
        allowed_code_hash_res.seg.size != code_hash_seg.size) {
      ckb_debug("disallow script because eoa code_hash is invalid");
      return GW_FATAL_INVALID_DATA;
    } else {
      is_eos_account = true;
      break;
    }
  }
  if (!is_eos_account) {
    /* check allowed contract list */
    mol_seg_t contract_list_seg =
      MolReader_RollupConfig_get_allowed_contract_type_hashes(&rollup_config_seg);
    len = MolReader_Byte32Vec_length(&contract_list_seg);

    for (uint32_t i = 0; i < len; i++) {
      mol_seg_res_t allowed_code_hash_res = MolReader_Byte32Vec_get(&contract_list_seg, i);
      if (memcmp(allowed_code_hash_res.seg.ptr, code_hash_seg.ptr,
        code_hash_seg.size) != 0) continue;
      if (allowed_code_hash_res.errno != MOL_OK ||
          allowed_code_hash_res.seg.size != code_hash_seg.size) {
        ckb_debug("disallow script because contract code_hash is invalid");
        return GW_FATAL_INVALID_DATA;
      } else {
        // check that contract'script must start with a 32 bytes rollup_script_hash
        mol_seg_t args_seg = MolReader_Script_get_args(&script_seg);
        mol_seg_t raw_args_seg = MolReader_Bytes_raw_bytes(&args_seg);
        if (raw_args_seg.size < 32) {
          ckb_debug("disallow contract script because args is less than 32 bytes");
          return GW_ERROR_INVALID_ACCOUNT_SCRIPT;
        }
        // check contract script short length
        if (memcmp(g_rollup_script_hash, raw_args_seg.ptr, 32) != 0) {
          ckb_debug("disallow contract script because args is not start with "
                    "rollup_script_hash");
          return GW_ERROR_INVALID_ACCOUNT_SCRIPT;
        }
      }
    }
  }
#pragma pop_macro("errno")

  //TODO: use create_account_from_script fn
  /* Same logic from State::create_account() */
  uint32_t id = gw_host->account_count; // tmp
  const uint8_t zero_nonce[GW_VALUE_BYTES] = {0};
  uint8_t account_key[GW_KEY_BYTES];
  
  // store(account_nonce_key -> zero_nonce)
  gw_build_account_field_key(id, GW_ACCOUNT_NONCE, account_key);
  gw_update_raw(account_key, zero_nonce);
  
  // store(script_hash_key -> script_hash)
  gw_build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH, account_key);
  gw_update_raw(account_key, script_hash);

  // store(script_hash -> account_id)
  uint8_t script_hash_to_id_key[GW_KEY_BYTES];
  uint8_t script_hash_to_id_value[GW_VALUE_BYTES] = {0};
  gw_build_script_hash_to_account_id_key(script_hash, script_hash_to_id_key);
  memcpy(script_hash_to_id_value, (uint8_t *)(&id), 4);
  gw_update_raw(script_hash_to_id_key, script_hash_to_id_value);

  // store(script hash -> script_hash)
  uint8_t short_script_hash_to_script_key[32] = {0};
  ret = gw_build_short_script_hash_to_script_hash_key(
    script_hash, GW_DEFAULT_SHORT_SCRIPT_HASH_LEN,
    short_script_hash_to_script_key
  );
  if (ret != 0) return ret;
  gw_update_raw(short_script_hash_to_script_key, script_hash);

  // insert script: store_data(script_hash -> script)
  ret = gw_store_data(script_len, script);
  if (ret != 0) return ret;

  // account_count++
  gw_host->account_count++;

  // return id
  *account_id_ptr = id;
  dbg_print("new account id = %d was created.", id);

  return ret;
}

// void create_account(uint8_t script_hash[GW_KEY_BYTES], uint32_t id) {
// }

// create account by script and return the new account id
uint32_t create_account_from_script(uint8_t *script, uint64_t script_len) {
  // uint8_t script_hash_type;
  // memcpy(&script_hash_type, script + 8, sizeof(uint8_t));
  // dbg_print("script_hash_type = %d", script_hash_type);
  // TODO:
  // if (script_hash_type != 1) { //TODO:
  //   dbg_print("AccountError::UnknownScript");
  // }
  
  // store_data(script_hash -> script)
  uint8_t script_hash[GW_KEY_BYTES];
  blake2b_hash(script_hash, script, script_len);
  gw_store_data(script_len, script);

  // create account with script_hash
  uint32_t id = gw_host->account_count;
  const uint8_t zero_nonce[GW_VALUE_BYTES] = {0};

  // store(account_nonce_key -> zero_nonce)
  uint8_t account_nonce_key[GW_KEY_BYTES];
  gw_build_account_field_key(id, GW_ACCOUNT_NONCE, account_nonce_key);
  gw_update_raw(account_nonce_key, zero_nonce);
  
  // store(script_hash_key -> script_hash)
  uint8_t account_script_hash_key[GW_KEY_BYTES];
  gw_build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH, account_script_hash_key);
  gw_update_raw(account_script_hash_key, script_hash);

  // store(script_hash -> account_id)
  uint8_t script_hash_to_id_key[GW_KEY_BYTES];
  uint8_t script_hash_to_id_value[GW_VALUE_BYTES] = {0};
  gw_build_script_hash_to_account_id_key(script_hash, script_hash_to_id_key);
  // FIXME: id may be more than 256
  memcpy(script_hash_to_id_value, (uint8_t *)(&id), 4);
  gw_update_raw(script_hash_to_id_key, script_hash_to_id_value);

  /* init short script hash -> script_hash */
  uint8_t short_script_hash_to_script_key[32] = {0};
  gw_build_short_script_hash_to_script_hash_key(
    script_hash, GW_DEFAULT_SHORT_SCRIPT_HASH_LEN,
    short_script_hash_to_script_key
  );
  gw_update_raw(short_script_hash_to_script_key, script_hash);

  // account_count++
  dbg_print("new account id = %d was created.", id);
  return gw_host->account_count++;
}

int init() {
  // init block_hash
  gw_host->block_hash = bytes32({7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7});

  // TODO: build RollupConfig, @see polyjuice-tests/src/helper.rs
  // init rollup_config
  const uint8_t rollup_config[] = {189, 1, 0, 0, 60, 0, 0, 0, 92, 0, 0, 0, 124, 0, 0, 0, 156, 0, 0, 0, 188, 0, 0, 0, 220, 0, 0, 0, 252, 0, 0, 0, 28, 1, 0, 0, 60, 1, 0, 0, 68, 1, 0, 0, 76, 1, 0, 0, 84, 1, 0, 0, 85, 1, 0, 0, 89, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 162, 22, 32, 241, 19, 215, 157, 151, 224, 246, 229, 9, 24, 60, 121, 13, 182, 82, 23, 174, 212, 233, 95, 5, 215, 202, 209, 205, 163, 74, 191, 27, 11};
  gw_host->rollup_config_size = sizeof(rollup_config);
  memcpy(gw_host->rollup_config, rollup_config, gw_host->rollup_config_size);
  
  
// TODO: build_script()
// build_script(g_script_code_hash, g_script_hash_type, script_args, SCRIPT_ARGS_LEN, &new_script_seg);

  uint32_t new_id = -1;
  // id = 0
  bytes reserved_account_script
    = from_hex("35000000100000003000000031000000a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a10100000000");
  new_id = create_account_from_script((uint8_t *)reserved_account_script.data(),
                                      reserved_account_script.size());
  // id = 1
  bytes ckb_account_script
    = from_hex("75000000100000003000000031000000a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a20140000000a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a90000000000000000000000000000000000000000000000000000000000000000");
  new_id = create_account_from_script((uint8_t *)ckb_account_script.data(),
                                      ckb_account_script.size());
  // id = 2
  bytes meta_account_script
     = from_hex("590000001000000030000000310000001620f113d79d97e0f6e509183c790db65217aed4e95f05d7cad1cda34abf1b0b0124000000a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a901000000");
  new_id = create_account_from_script((uint8_t *)meta_account_script.data(),
                                      meta_account_script.size());
  // id = 3
  bytes block_producer_script = from_hex("69000000100000003000000031000000aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa0134000000a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a99999999999999999999999999999999999999999");
  new_id = create_account_from_script((uint8_t *)block_producer_script.data(),
                                      block_producer_script.size());
  // id = 4
  bytes build_eth_l2_script = from_hex("69000000100000003000000031000000aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa0134000000a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a90101010101010101010101010101010101010101");

  new_id = create_account_from_script((uint8_t *)build_eth_l2_script.data(),
                                      build_eth_l2_script.size());
  mock_mint_sudt(1, new_id, 40000);
  
  // TODO: init destructed key
  const uint8_t zero_nonce[32] = {0};
  const uint8_t poly_destructed_key[32] = {5, 0, 0, 0, 255, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0};
  gw_update_raw(poly_destructed_key, zero_nonce);

  print_state();

  return 0;
}
