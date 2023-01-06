import { HexNumber } from "@ckb-lumos/base";

// all the system exit code mappings
export interface ExitCode {
  code: number;
  type: string;
  message: string;
}

export interface ExitCodeMapping {
  [key: string]: ExitCode;
}
// evmc error code
// From https://github.com/ethereum/evmc/blob/v9.0.0/include/evmc/evmc.h#L212
export const EVMC_EXIT_CODE_MAPPING = {
  success: {
    code: 0,
    type: "SUCCESS",
    message: "success",
  },
  failure: {
    code: 1,
    type: "FAILURE",
    message: "failure",
  },
  revert: {
    code: 2,
    type: "REVERT",
    message: "revert",
  },
  outOfGas: {
    code: 3,
    type: "OUT_OF_GAS",
    message: "out of gas",
  },
  invalidInstruction: {
    code: 4,
    type: "INVALID_INSTRUCTION",
    message: "invalid instruction",
  },
  undefinedInstruction: {
    code: 5,
    type: "UNDEFINED_INSTRUCTION",
    message: "undefined instruction",
  },
  stackOverflow: {
    code: 6,
    type: "STACK_OVERFLOW",
    message: "stack overflow",
  },
  stackUnderflow: {
    code: 7,
    type: "STACK_UNDERFLOW",
    message: "stack underflow",
  },
  badJumpDestination: {
    code: 8,
    type: "BAD_JUMP_DESTINATION",
    message: "bad jump destination",
  },
  invalidMemoryAccess: {
    code: 9,
    type: "INVALID_MEMORY_ACCESS",
    message: "invalid memory access",
  },
  callDepthExceeded: {
    code: 10,
    type: "CALL_DEPTH_EXCEEDED",
    message: "call depth exceeded",
  },
  staticModeViolation: {
    code: 11,
    type: "STATIC_MODE_VIOLATION",
    message: "static mode violation",
  },
  precompileFailure: {
    code: 12,
    type: "PRECOMPILE_FAILURE",
    message: "precompile failure",
  },
  contractValidationFailure: {
    code: 13,
    type: "CONTRACT_VALIDATION_FAILURE",
    message: "contract validation failure",
  },
  argumentOutOfRange: {
    code: 14,
    type: "ARGUMENT_OUT_OF_RANGE",
    message: "argument out of range",
  },
  wasmUnreachableInstruction: {
    code: 15,
    type: "WASM_UNREACHABLE_INSTRUCTION",
    message: "wasm unreachable instruction",
  },
  wasmTrap: {
    code: 16,
    type: "WASM_TRAP",
    message: "wasm trap",
  },
  insufficientBalance: {
    code: 17,
    type: "INSUFFICIENT_BALANCE",
    message: "insufficient balance",
  },
  internalError: {
    code: -1,
    type: "INTERNAL_ERROR",
    message: "internal error",
  },
  rejected: {
    code: -2,
    type: "REJECTED",
    message: "rejected",
  },
  outOfMemory: {
    code: -3,
    type: "OUT_OF_MEMORY",
    message: "out of memory",
  },
};

// polyjuice error code
// From https://github.com/godwokenrises/godwoken-polyjuice/blob/main/c/polyjuice_errors.h
export const POLYJUICE_EXIT_CODE_MAPPING = {
  fatalPolyjuice: {
    code: -50,
    type: "FATAL_POLYJUICE",
    message: "fatal polyjuice",
  },
  fatalPrecompiledContracts: {
    code: -51,
    type: "FATAL_PRECOMPILED_CONTRACTS",
    message: "fatal precompiled contracts",
  },
  errorModExp: {
    code: -80,
    type: "ERROR_MOD_EXP",
    message: "error mod exp",
  },
  errorBlake2fInvalidInputLength: {
    code: -81,
    type: "ERROR_BLAKE2F_INVALID_INPUT_LENGTH",
    message: "error blake2f invalid input length",
  },
  errorBlake2fInvalidFinalFlag: {
    code: -82,
    type: "ERROR_BLAKE2F_INVALID_FINAL_FLAG",
    message: "error blake2f invalid final flag",
  },
  errorBn256Add: {
    code: -83,
    type: "ERROR_BN256_ADD",
    message: "error bn256 add",
  },
  errorBn256ScalarMul: {
    code: -84,
    type: "ERROR_BN256_SCALAR_MUL",
    message: "error bn256 scalar mul",
  },
  errorBn256Pairing: {
    code: -85,
    type: "ERROR_BN256_PAIRING",
    message: "error bn256 pairing",
  },
  errorBn256InvalidPoint: {
    code: -86,
    type: "ERROR_BN256_INVALID_POINT",
    message: "error bn256 invalid point",
  },
  errorBalanceOfAnySudt: {
    code: -87,
    type: "ERROR_BALANCE_OF_ANY_SUDT",
    message: "error balance of any sudt",
  },
  errorTransferToAnySudt: {
    code: -88,
    type: "ERROR_TRANSFER_TO_ANY_SUDT",
    message: "error transfer to any sudt",
  },
  errorRecoverAccount: {
    code: -89,
    type: "ERROR_RECOVER_ACCOUNT",
    message: "error recover account",
  },
  errorTotalSupplyOfAnySudt: {
    code: -91,
    type: "ERROR_TOTAL_SUPPLY_OF_ANY_SUDT",
    message: "error total supply of any sudt",
  },
  errorContractAddressCollision: {
    code: -92,
    type: "ERROR_CONTRACT_ADDRESS_COLLISION",
    message: "error contract address collision",
  },
  errorInsufficientGasLimit: {
    code: -93,
    type: "ERROR_INSUFFICIENT_GAS_LIMIT",
    message: "error insufficient gas limit",
  },
  errorNativeTokenTransfer: {
    code: -94,
    type: "ERROR_NATIVE_TOKEN_TRANSFER",
    message: "error native token transfer",
  },
};

// godwoken error code
// From https://github.com/godwokenrises/godwoken/blob/develop/gwos/c/gw_errors.h
export const GODWOKEN_EXIT_CODE_MAPPING = {
  vmReachedMaxCycles: {
    code: -1,
    type: "VM_REACHED_MAX_CYCLES",
    message: "vm reached max cycles",
  },

  /* Data Fatals 5x */
  gwFatalBufferOverflow: {
    code: 50,
    type: "GW_FATAL_BUFFER_OVERFLOW",
    message: "gw fatal buffer overflow",
  },
  gwFatalInvalidContext: {
    code: 51,
    type: "GW_FATAL_INVALID_CONTEXT",
    message: "gw fatal invalid context",
  },
  gwFatalInvalidData: {
    code: 52,
    type: "GW_FATAL_INVALID_DATA",
    message: "gw fatal invalid data",
  },
  gwFatalMismatchReturnData: {
    code: 53,
    type: "GW_FATAL_MISMATCH_RETURN_DATA",
    message: "gw fatal mismatch return data",
  },
  gwFatalUnknownArgs: {
    code: 54,
    type: "GW_FATAL_UNKNOWN_ARGS",
    message: "gw fatal unknown args",
  },
  gwFatalInvalidSudtScript: {
    code: 55,
    type: "GW_FATAL_INVALID_SUDT_SCRIPT",
    message: "gw fatal invalid sudt script",
  },
  gwFatalInvalidCheckPoint: {
    code: 56,
    type: "GW_FATAL_INVALID_CHECK_POINT",
    message: "gw fatal invalid check point",
  },

  /* Notfound Fatals 6x */
  gwFatalDataCellNotFound: {
    code: 60,
    type: "GW_FATAL_DATA_CELL_NOT_FOUND",
    message: "gw fatal data cell not found",
  },
  gwFatalStateKeyNotFound: {
    code: 61,
    type: "GW_FATAL_STATE_KEY_NOT_FOUND",
    message: "gw fatal state key not found",
  },
  gwFatalSignatureCellNotFound: {
    code: 62,
    type: "GW_FATAL_SIGNATURE_CELL_NOT_FOUND",
    message: "gw fatal signature cell not found",
  },
  gwFatalScriptNotFound: {
    code: 63,
    type: "GW_FATAL_SCRIPT_NOT_FOUND",
    message: "gw fatal script not found",
  },

  /* Merkle Fatals 7x */
  gwFatalSmtVerify: {
    code: 70,
    type: "GW_FATAL_SMT_VERIFY",
    message: "gw fatal smt verify",
  },
  gwFatalSmtFetch: {
    code: 71,
    type: "GW_FATAL_SMT_FETCH",
    message: "gw fatal smt fetch",
  },
  gwFatalSmtStore: {
    code: 72,
    type: "GW_FATAL_SMT_STORE",
    message: "gw fatal smt store",
  },
  gwFatalSmtCalculateRoot: {
    code: 73,
    type: "GW_FATAL_SMT_CALCULATE_ROOT",
    message: "gw fatal smt calculate root",
  },

  /* Syscall Errors */
  gwErrorDuplicatedScriptHash: {
    code: 80,
    type: "GW_ERROR_DUPLICATED_SCRIPT_HASH",
    message: "gw error duplicated script hash",
  },
  gwErrorUnknownScriptCodeHash: {
    code: 81,
    type: "GW_ERROR_UNKNOWN_SCRIPT_CODE_HASH",
    message: "gw error unknown script code hash",
  },
  gwErrorInvalidAccountScript: {
    code: 82,
    type: "GW_ERROR_INVALID_ACCOUNT_SCRIPT",
    message: "gw error invalid account script",
  },
  gwErrorNotFound: {
    code: 83,
    type: "GW_ERROR_NOT_FOUND",
    message: "gw error not found",
  },
  gwErrorRecover: {
    code: 84,
    type: "GW_ERROR_RECOVER",
    message: "gw error recover",
  },
  gwErrorAccountNotExists: {
    code: 85,
    type: "GW_ERROR_ACCOUNT_NOT_EXISTS",
    message: "gw error account not exists",
  },
  gwUnimplemented: {
    code: 86,
    type: "GW_UNIMPLEMENTED",
    message: "gw unimplemented",
  },

  /* sUDT errors */
  gwSudtErrorInsufficientBalance: {
    code: 92,
    type: "GW_SUDT_ERROR_INSUFFICIENT_BALANCE",
    message: "gw sudt error insufficient balance",
  },
  gwSudtErrorAmountOverflow: {
    code: 93,
    type: "GW_SUDT_ERROR_AMOUNT_OVERFLOW",
    message: "gw sudt error amount overflow",
  },
  gwSudtErrorToAddr: {
    code: 94,
    type: "GW_SUDT_ERROR_TO_ADDR",
    message: "gw sudt error to addr",
  },
  gwSudtErrorAccountNotExists: {
    code: 95,
    type: "GW_SUDT_ERROR_ACCOUNT_NOT_EXISTS",
    message: "gw sudt error account not exists",
  },

  /* Registry error */
  gwRegistryErrorDuplicateMapping: {
    code: 101,
    type: "GW_REGISTRY_ERROR_DUPLICATE_MAPPING",
    message: "gw registry error duplicate mapping",
  },
};

export function matchExitCode(
  exitCode: HexNumber | number,
  mapping: ExitCodeMapping
): ExitCode | null {
  let code: number;
  if (typeof exitCode === "number") {
    code = exitCode;
  } else {
    code = exitCodeHexToNumber(exitCode);
  }

  const exitCodes = Object.entries(mapping)
    .filter(([_key, value]) => value.code === code)
    .map(([_key, exitCode]) => exitCode);

  if (exitCodes.length === 0) {
    return null;
  }

  return exitCodes[0];
}

// the exit_code in godwoken is always an i8 type.
export function exitCodeHexToNumber(hex: HexNumber): number {
  let num = parseInt(hex, 16);
  if (num > 127) {
    num = num - 256;
  }
  return num;
}

// the exit_code in godwoken is always an i8 type.
export function exitCodeNumberToHex(num: number): HexNumber {
  if (num < 0) {
    num = num + 256;
  }
  return "0x" + num.toString(16);
}
