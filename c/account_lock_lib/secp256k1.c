#include "blake2b.h"
#include "secp256k1_helper.h"

/* constants */
#define SIGNATURE_SIZE 65
#define PUBKEY_SIZE 65
#define RECID_INDEX 64

/* errors */
#define ERROR_SECP_SERIALIZE_PUBKEY 140
#define ERROR_SECP_RECOVER_PUBKEY 141
#define ERROR_SECP_PARSE_SIGNATURE 142

int recover_secp256k1_uncompressed_key(uint8_t message[32],
                                       uint8_t signature[SIGNATURE_SIZE],
                                       uint8_t output_uncompressed_pubkey[65]) {

  // Setup secp256k1 data
  secp256k1_context context;
  uint8_t secp_data[CKB_SECP256K1_DATA_SIZE];
  int ret = ckb_secp256k1_custom_verify_only_initialize(&context, secp_data);
  if (ret != 0) {
    printf("Error occured when initializing secp256k1");
    return ret;
  }

  secp256k1_ecdsa_recoverable_signature recoverable_signature;
  if (secp256k1_ecdsa_recoverable_signature_parse_compact(
          &context, &recoverable_signature, signature,
          signature[RECID_INDEX]) == 0) {
    printf("Error occured when parsing recoverable signature");
    return ERROR_SECP_PARSE_SIGNATURE;
  }

  // Recover pubkey
  secp256k1_pubkey pubkey;
  if (secp256k1_ecdsa_recover(&context, &pubkey, &recoverable_signature,
                              message) != 1) {
    printf("Error occured when recovering pubkey");
    return ERROR_SECP_RECOVER_PUBKEY;
  }

  // Check pubkey hash
  size_t pubkey_size = PUBKEY_SIZE;
  if (secp256k1_ec_pubkey_serialize(&context, output_uncompressed_pubkey,
                                    &pubkey_size, &pubkey,
                                    SECP256K1_EC_UNCOMPRESSED) != 1) {
    printf("Error occued when serializing pubkey");
    return ERROR_SECP_SERIALIZE_PUBKEY;
  }

  return CKB_SUCCESS;
}
