// From https://github.com/ethereum/evmc/blob/v9.0.0/include/evmc/evmc.h#L212

import abiCoder, { AbiCoder } from "web3-eth-abi";
import { RpcError, TransactionExecutionError } from "./error";
import { HexNumber, HexString } from "@ckb-lumos/base";
import { logger } from "../base/logger";
import { ErrorTxReceipt, isErrorTxReceipt } from "@godwoken-web3/godwoken";
import { INTERNAL_ERROR } from "./error-code";

const GODWOKEN_SERVER_ERROR_MESSAGE_PREFIX = "JSONRPCError: server error ";

const REVERT_SELECTOR: string = "0x08c379a0";
const PANIC_SELECTOR: string = "0x4e487b71";

/**
 * Determine whether the error is coming from Godwoken
 */
export function isGwError(err: any): boolean {
  const message: string = err?.message || err;
  return (
    message != null &&
    typeof message === "string" &&
    message.startsWith(GODWOKEN_SERVER_ERROR_MESSAGE_PREFIX)
  );
}

/**
 * Parse the given Godwoken error, translate into RpcError and then throw it
 *
 * @param gwJsonRpcError
 *
 * @throws RpcError
 */
export function handleGwError(gwJsonRpcError: any) {
  if (!isGwError(gwJsonRpcError)) {
    throw gwJsonRpcError;
  }

  const message: string = gwJsonRpcError?.message || gwJsonRpcError;
  const err = JSON.parse(
    message.slice(GODWOKEN_SERVER_ERROR_MESSAGE_PREFIX.length)
  );

  if (isTransactionErrorInvalidExitCode(err)) {
    // Example:
    // ```rust
    // {
    //     code: INVALID_REQUEST,
    //     message: TransactionError::InvalidExitCode(run_result.exit_code).to_string(),
    //     data: Some(Box::new(ErrorTxReceipt::from(receipt))),
    // };
    // ```
    //
    // For `TransactionError::InvalidExitCode`, the `data` field should always be `ErrorTxReceipt`
    if (isErrorTxReceipt(err.data)) {
      handleErrorTxReceipt(err.data as ErrorTxReceipt);
    }
  } else if (message.startsWith("request to")) {
    // Connection error
    throw new Error(message);
  }

  throw new RpcError(
    err.code || INTERNAL_ERROR,
    err.message || gwJsonRpcError.toString(),
    err.data
  );
}

/**
 * Throw TransactionExecutionError transferred from ErrorTxReceipt
 *
 * @param errorTxReceipt
 * @throws TransactionExecutionError
 */
export function handleErrorTxReceipt(errorTxReceipt: ErrorTxReceipt) {
  const GW_VM_MAX_CYCLE_EXIT_CODE = "0xff";

  if (errorTxReceipt.exit_code === GW_VM_MAX_CYCLE_EXIT_CODE) {
    throw new TransactionExecutionError("out of gas");
  }

  // fallback to general revert error
  handleRevertError(errorTxReceipt);
}

function handleRevertError(errorTxReceipt: ErrorTxReceipt) {
  const returnData = errorTxReceipt.return_data;
  const message = parseReturnData(returnData);
  if (errorTxReceipt.return_data.length > 2) {
    throw new TransactionExecutionError(message, returnData);
  } else {
    throw new TransactionExecutionError(message);
  }
}

/**
 * Resolves the abi-encoded panic reason or revert reason.
 *
 * @param {HexString} returnData The returned data in [ErrorTxReceipt](https://github.com/nervosnetwork/godwoken/blob/c4be58f30744aef83717e2a12d60fe4d50b165ab/crates/jsonrpc-types/src/godwoken.rs#L1310-L1317)
 *
 * @see {@link https://docs.soliditylang.org/en/v0.8.13/control-structures.html#panic-via-assert-and-error-via-require}
 */
export function parseReturnData(returnData: HexString): string {
  if (returnData.slice(0, REVERT_SELECTOR.length) === REVERT_SELECTOR) {
    return unpackRevert(returnData);
  } else if (returnData.slice(0, PANIC_SELECTOR.length) === PANIC_SELECTOR) {
    return unpackPanic(returnData);
  } else {
    return "execution reverted";
  }
}

/**
 * Resolves the abi-encoded revert reason. According to the solidity
 * spec https://solidity.readthedocs.io/en/latest/control-structures.html#revert,
 * the provided revert reason is abi-encoded as if it were a call to a function
 * `Error(string)`. So it's a special tool for it.
 *
 * @param {HexString} returnData The returned data in [ErrorTxReceipt](https://github.com/nervosnetwork/godwoken/blob/c4be58f30744aef83717e2a12d60fe4d50b165ab/crates/jsonrpc-types/src/godwoken.rs#L1310-L1317)
 *
 * @return {(string)} The wrapped revert reason
 *
 * @see {@link https://github.com/ethereum/go-ethereum/blob/420b78659bef661a83c5c442121b13f13288c09f/accounts/abi/abi.go#L262-L279}
 *
 * @example
 * // returns "execution reverted"
 * Solidity `revert()`
 *
 * @example
 * // returns "execution reverted"
 * Solidity `revert(CustomError({reason: "reason"}))`
 *
 * @example
 * // returns "execution reverted: "
 * Solidity `revert("")`
 *
 * @example
 * // returns "execution reverted: reason"
 * Solidity `revert("reason")`
 */
export function unpackRevert(returnData: HexString): string {
  if (returnData.slice(0, REVERT_SELECTOR.length) !== REVERT_SELECTOR) {
    return "execution reverted";
  }
  if (returnData.length === REVERT_SELECTOR.length) {
    return "execution reverted: ";
  }

  const abi = abiCoder as unknown as AbiCoder;
  try {
    const parsedArgs = abi.decodeParameters(
      ["string"],
      returnData.slice(REVERT_SELECTOR.length)
    );
    const reason = parsedArgs[0];
    return `execution reverted: ${reason}`;
  } catch (err: any) {
    logger.error(
      `fail to decode revert reason, error: ${err}, returnData: ${returnData}`
    );
    return "execution reverted";
  }
}

/**
 * Resolves the abi-encoded panicked reason.
 *
 * @see {@link https://docs.soliditylang.org/en/v0.8.13/control-structures.html#panic-via-assert-and-error-via-require}
 */
export function unpackPanic(returnData: HexString): string {
  if (returnData.slice(0, PANIC_SELECTOR.length) !== PANIC_SELECTOR) {
    return "execution reverted";
  }

  // From https://github.com/NomicFoundation/hardhat/blob/ef14cb35114b3e6b28ed697fe74049c38695afb3/packages/hardhat-core/src/internal/hardhat-network/stack-traces/panic-errors.ts#L13-L34
  const panicCodeToReason: { [key: string]: string } = {
    "0x1": "Assertion error",
    "0x11":
      "Arithmetic operation underflowed or overflowed outside of an unchecked block",
    "0x12": "Division or modulo division by zero",
    "0x21":
      "Tried to convert a value into an enum, but the value was too big or negative",
    "0x22": "Incorrectly encoded storage byte array",
    "0x31": ".pop() was called on an empty array",
    "0x32": "Array accessed at an out-of-bounds or negative index",
    "0x41":
      "Too much memory was allocated, or an array was created that is too large",
    "0x51": "Called a zero-initialized variable of internal function type",
  };

  const abi = abiCoder as unknown as AbiCoder;
  try {
    const parsedArgs = abi.decodeParameters(
      ["uint256"],
      returnData.slice(PANIC_SELECTOR.length)
    );
    const code: HexNumber = "0x" + BigInt(parsedArgs[0]).toString(16);
    const reason = panicCodeToReason[code];
    if (reason != null) {
      return `execution reverted: panic code ${code} (${reason})`;
    } else {
      return `execution reverted: panic code ${code}`;
    }
  } catch (err: any) {
    logger.error(
      `fail to decode panic error code, error: ${err}, returnData: ${returnData}`
    );
    return "execution reverted";
  }
}

/**
 * Returns whether the error is a transaction execution error from Godwoken
 */
function isTransactionErrorInvalidExitCode(err: any): boolean {
  const GODWOKEN_TRANSACTION_ERROR_INVALID_EXIT_CODE_PREFIX =
    "invalid exit code ";
  const message: string = err?.message;
  return (
    message != null &&
    typeof message === "string" &&
    message.startsWith(GODWOKEN_TRANSACTION_ERROR_INVALID_EXIT_CODE_PREFIX)
  );
}
