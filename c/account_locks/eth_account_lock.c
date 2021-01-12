#include "blake2b.h"
#include "ckb_syscalls.h"
#include "gw_syscalls.h"
#include "secp256k1_helper.h"
#include "sha3/sha3.h"
#include "stdio.h"

/* Defines */
#define RECID_INDEX 64
#define BLAKE2B_BLOCK_SIZE 32
#define BLAKE160_SIZE 20
#define SCRIPT_SIZE 32768
#define MAX_WITNESS_SIZE 32768
#define PUBKEY_SIZE 65
/* Errors */
#define ERROR_ARGUMENTS_LEN -1
#define ERROR_ENCODING -2
#define ERROR_SYSCALL -3
#define ERROR_SECP_RECOVER_PUBKEY -11
#define ERROR_SECP_VERIFICATION -12
#define ERROR_SECP_PARSE_PUBKEY -13
#define ERROR_SECP_PARSE_SIGNATURE -14
#define ERROR_SECP_SERIALIZE_PUBKEY -15
#define ERROR_SCRIPT_TOO_LONG -21
#define ERROR_WITNESS_SIZE -22
#define ERROR_INCORRECT_SINCE_FLAGS -23
#define ERROR_INCORRECT_SINCE_VALUE -24
#define ERROR_PUBKEY_BLAKE160_HASH -31
/* Others */
#define ETH_SIGNING_PREFIX                                                     \
  ("\x19"                                                                      \
   "Ethereum Signed Message:\n32")

int load_pubkey_hash(uint8_t pubkey_hash[BLAKE160_SIZE]) {
  unsigned char script[SCRIPT_SIZE];
  uint64_t len = SCRIPT_SIZE;
  int ret = ckb_load_script(script, &len, 0);
  if (ret != CKB_SUCCESS) {
    return ERROR_SYSCALL;
  }
  if (len > SCRIPT_SIZE) {
    return ERROR_SCRIPT_TOO_LONG;
  }
  mol_seg_t script_seg;
  script_seg.ptr = (uint8_t *)script;
  script_seg.size = len;

  if (MolReader_Script_verify(&script_seg, false) != MOL_OK) {
    return ERROR_ENCODING;
  }

  mol_seg_t args_seg = MolReader_Script_get_args(&script_seg);
  mol_seg_t args_bytes_seg = MolReader_Bytes_raw_bytes(&args_seg);
  if (args_bytes_seg.size != BLAKE160_SIZE) {
    return ERROR_ARGUMENTS_LEN;
  }
  memcpy(pubkey_hash, args_bytes_seg.ptr, BLAKE160_SIZE);
  return CKB_SUCCESS;
}

/* Extract lock from WitnessArgs */
int extract_witness_lock(uint8_t *witness, uint64_t len,
                         mol_seg_t *lock_bytes_seg) {
  mol_seg_t witness_seg;
  witness_seg.ptr = witness;
  witness_seg.size = len;

  if (MolReader_WitnessArgs_verify(&witness_seg, false) != MOL_OK) {
    return ERROR_ENCODING;
  }
  mol_seg_t lock_seg = MolReader_WitnessArgs_get_lock(&witness_seg);

  if (MolReader_BytesOpt_is_none(&lock_seg)) {
    return ERROR_ENCODING;
  }
  *lock_bytes_seg = MolReader_Bytes_raw_bytes(&lock_seg);
  return CKB_SUCCESS;
}

int load_unlock_account_witness(uint8_t temp[MAX_WITNESS_SIZE],
                                mol_seg_t *unlock_account_witness_seg) {
  // Load the first witness, or the witness of the same index as the first input
  // using current script.
  uint64_t witness_len = MAX_WITNESS_SIZE;
  int ret = ckb_load_witness(temp, &witness_len, 0, 0, CKB_SOURCE_GROUP_INPUT);
  if (ret != CKB_SUCCESS) {
    return ERROR_SYSCALL;
  }

  if (witness_len > MAX_WITNESS_SIZE) {
    return ERROR_WITNESS_SIZE;
  }

  // We will treat the first witness as WitnessArgs object, and extract the lock
  // field from the object.
  ret = extract_witness_lock(temp, witness_len, unlock_account_witness_seg);
  if (ret != 0) {
    return ERROR_ENCODING;
  }

  if (MolReader_UnlockAccountWitness_verify(unlock_account_witness_seg,
                                            false) != MOL_OK) {
    return ERROR_ENCODING;
  }

  return CKB_SUCCESS;
}

int recover_pubkey(unsigned char recovered_pubkey[PUBKEY_SIZE],
                   unsigned char sig[65],
                   unsigned char msg[BLAKE2B_BLOCK_SIZE]) {
  secp256k1_context context;
  uint8_t secp_data[CKB_SECP256K1_DATA_SIZE];
  int ret = ckb_secp256k1_custom_load_data(secp_data);
  if (ret != 0) {
    return ret;
  }
  ret = ckb_secp256k1_custom_verify_only_initialize(&context, secp_data);
  if (ret != 0) {
    return ret;
  }

  secp256k1_ecdsa_recoverable_signature signature;
  if (secp256k1_ecdsa_recoverable_signature_parse_compact(
          &context, &signature, sig, sig[RECID_INDEX]) == 0) {
    return ERROR_SECP_PARSE_SIGNATURE;
  }

  unsigned char data[sizeof(ETH_SIGNING_PREFIX) - 1 + BLAKE2B_BLOCK_SIZE] =
      ETH_SIGNING_PREFIX;
  memcpy(data + sizeof(ETH_SIGNING_PREFIX) - 1, msg, BLAKE2B_BLOCK_SIZE);
  struct ethash_h256 signing_message = {0};
  SHA3_256(&signing_message, data, sizeof(data));

  // From the recoverable signature, we can derive the public key used.
  secp256k1_pubkey pubkey;
  if (secp256k1_ecdsa_recover(&context, &pubkey, &signature,
                              signing_message.b) != 1) {
    return ERROR_SECP_RECOVER_PUBKEY;
  }

  // Let's serialize the signature first, then generate the blake2b hash.
  size_t pubkey_size = PUBKEY_SIZE;
  if (secp256k1_ec_pubkey_serialize(&context, recovered_pubkey, &pubkey_size,
                                    &pubkey, SECP256K1_EC_UNCOMPRESSED) != 1) {
    return ERROR_SECP_SERIALIZE_PUBKEY;
  }

  return CKB_SUCCESS;
}

int main() {
  /* Load pubkey hash */
  uint8_t pubkey_hash[BLAKE160_SIZE] = {0};
  int ret = load_pubkey_hash(pubkey_hash);
  if (ret != 0) {
    return ret;
  }
  /* Load unlock account witness */
  uint8_t witness[MAX_WITNESS_SIZE] = {0};
  mol_seg_t unlock_account_witness_seg = {0};
  ret = load_unlock_account_witness(witness, &unlock_account_witness_seg);
  if (ret != 0) {
    return ret;
  }

  mol_seg_t message_seg =
      MolReader_UnlockAccountWitness_get_message(&unlock_account_witness_seg);
  mol_seg_t signature_seg =
      MolReader_UnlockAccountWitness_get_signature(&unlock_account_witness_seg);

  uint8_t recovered_pubkey[PUBKEY_SIZE] = {0};
  ret = recover_pubkey(recovered_pubkey, signature_seg.ptr, message_seg.ptr);
  if (ret != 0) {
    return ret;
  }

  struct ethash_h256 recovered_pubkey_hash = {0};
  SHA3_256(&recovered_pubkey_hash, recovered_pubkey + 1, PUBKEY_SIZE - 1);
  if (memcmp(pubkey_hash, recovered_pubkey_hash.b + 12, BLAKE160_SIZE) != 0) {
    return ERROR_PUBKEY_BLAKE160_HASH;
  }

  return CKB_SUCCESS;
}
