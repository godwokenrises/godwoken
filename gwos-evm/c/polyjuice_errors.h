
#ifndef POLYJUICE_ERRORS_H
#define POLYJUICE_ERRORS_H

/* Fatals in polyjuice: [-50, -80)
 * Fatals represents errors that shouldn't be recovered
 */
#define FATAL_POLYJUICE             -50
#define FATAL_PRECOMPILED_CONTRACTS -51

/* Normal errors in polyjuice */
#define ERROR_MOD_EXP                           -80
#define ERROR_BLAKE2F_INVALID_INPUT_LENGTH      -81
#define ERROR_BLAKE2F_INVALID_FINAL_FLAG        -82
#define ERROR_BN256_ADD                         -83
#define ERROR_BN256_SCALAR_MUL                  -84
#define ERROR_BN256_PAIRING                     -85
#define ERROR_BN256_INVALID_POINT               -86
#define ERROR_BALANCE_OF_ANY_SUDT               -87
#define ERROR_TRANSFER_TO_ANY_SUDT              -88
#define ERROR_RECOVER_ACCOUNT                   -89

#define ERROR_TOTAL_SUPPLY_OF_ANY_SUDT          -91
#define ERROR_CONTRACT_ADDRESS_COLLISION        -92
#define ERROR_INSUFFICIENT_GAS_LIMIT            -93
#define ERROR_NATIVE_TOKEN_TRANSFER             -94

#endif // POLYJUICE_ERRORS_H
