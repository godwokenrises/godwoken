import { HexString, HexNumber } from "@ckb-lumos/base";
import { logger } from "../base/logger";
import { EntryPointContract } from "./entrypoint";
import { decodeGaslessPayload } from "./payload";

/**
 * Determine whether the eth transaction is a gasless transaction.
 */
export function isGaslessTransaction(
  {
    to: toAddress,
    gasPrice,
    data: inputData,
  }: {
    to: HexString;
    gasPrice: HexNumber;
    data: HexString;
  },
  entrypointContract: EntryPointContract
): boolean {
  // check if gas price is 0
  if (BigInt(gasPrice) != 0n) {
    return false;
  }

  // check if to == entrypoint
  if (toAddress != entrypointContract.address) {
    return false;
  }

  // check input data is GaslessPayload(can be decoded)
  try {
    decodeGaslessPayload(inputData);
  } catch (error: any) {
    logger.debug(
      "[isGaslessTransaction] try to decode gasless payload failed,",
      error.message
    );
    return false;
  }

  return true;
}
