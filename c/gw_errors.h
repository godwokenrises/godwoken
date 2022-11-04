#ifndef GW_ERRORS_H_
#define GW_ERRORS_H_

/* Godwoken Errors
   The exit code of CKB-VM is -128 ~ 127
   The 1 ~ 127 is used by Godwoken runtime(validator_utils.h &
   generator_utils.h). (To avoid conflict with backend we actually using 50 ~
   127) The Backend VM layer such as Polyjuice uses -1 ~ -128

   In Godwoken runtime, we seperate errors into Fatal & Errors,
   Fatals represents errors that shouldn't be recovered by user programs,
    typically caused by lack of validation context.
   Errors represents the syscall errors caused by the user input.
 */

/* Data Fatals 5x */
#define GW_FATAL_BUFFER_OVERFLOW 50
#define GW_FATAL_INVALID_CONTEXT 51
#define GW_FATAL_INVALID_DATA 52
#define GW_FATAL_MISMATCH_RETURN_DATA 53
#define GW_FATAL_UNKNOWN_ARGS 54
#define GW_FATAL_INVALID_SUDT_SCRIPT 55
#define GW_FATAL_INVALID_CHECK_POINT 56

/* Notfound Fatals 6x */
#define GW_FATAL_DATA_CELL_NOT_FOUND 60
#define GW_FATAL_STATE_KEY_NOT_FOUND 61
#define GW_FATAL_SIGNATURE_CELL_NOT_FOUND 62
#define GW_FATAL_SCRIPT_NOT_FOUND 63

/* Merkle Fatals 7x */
#define GW_FATAL_SMT_VERIFY 70
#define GW_FATAL_SMT_FETCH 71
#define GW_FATAL_SMT_STORE 72
#define GW_FATAL_SMT_CALCULATE_ROOT 73

/* Syscall Errors */
#define GW_ERROR_DUPLICATED_SCRIPT_HASH 80
#define GW_ERROR_UNKNOWN_SCRIPT_CODE_HASH 81
#define GW_ERROR_INVALID_ACCOUNT_SCRIPT 82
#define GW_ERROR_NOT_FOUND 83
#define GW_ERROR_RECOVER 84
#define GW_ERROR_ACCOUNT_NOT_EXISTS 85
#define GW_UNIMPLEMENTED 86

/* sUDT errors */
#define GW_SUDT_ERROR_INSUFFICIENT_BALANCE 92
#define GW_SUDT_ERROR_AMOUNT_OVERFLOW 93
#define GW_SUDT_ERROR_TO_ADDR 94
#define GW_SUDT_ERROR_ACCOUNT_NOT_EXISTS 95

/* Registry error */
#define GW_REGISTRY_ERROR_DUPLICATE_MAPPING 101

#endif
