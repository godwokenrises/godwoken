import { HexNumber, HexString } from "@ckb-lumos/base";
import { AbiCoder } from "web3-eth-abi";
const abiCoder = require("web3-eth-abi") as AbiCoder;

export interface UserOperation {
  callContract: HexString;
  callData: HexString;
  callGasLimit: HexNumber;
  verificationGasLimit: HexNumber;
  maxFeePerGas: HexNumber;
  maxPriorityFeePerGas: HexNumber;
  paymasterAndData: HexString;
}

export const USER_OPERATION_ABI_TYPE = {
  UserOperation: {
    callContract: "address",
    callData: "bytes",
    callGasLimit: "uint256",
    verificationGasLimit: "uint256",
    maxFeePerGas: "uint256",
    maxPriorityFeePerGas: "uint256",
    paymasterAndData: "bytes",
  },
};

// first 4 bytes of keccak hash of handleOp((address,bytes,uint256,uint256,uint256,uint256,bytes))
const ENTRYPOINT_HANDLE_OP_SELECTOR: HexString = "fb4350d8";

// Note: according to the godwoken gasless transaction specs:
// a gasless transaction's input data is a serialized Gasless payload.
// gasless payload = ENTRYPOINT_HANDLE_OP_SELECTOR + abiEncode(UserOperation)
// which is also the call data of calling entrypoint contract with handleOp(UserOperation userOp) method
export function decodeGaslessPayload(inputData: HexString): UserOperation {
  if (inputData.length < 10) {
    throw new Error(
      `invalid gasless tx.data length ${inputData.length}, expect at least 10`
    );
  }

  // check first 4 bytes
  const fnSelector = inputData.slice(2, 10);
  if (fnSelector !== ENTRYPOINT_HANDLE_OP_SELECTOR) {
    throw new Error(
      `invalid gasless tx.data fn selector ${fnSelector}, expect ${ENTRYPOINT_HANDLE_OP_SELECTOR}`
    );
  }

  const userOpData = "0x" + inputData.slice(10);
  const decoded = abiCoder.decodeParameter(USER_OPERATION_ABI_TYPE, userOpData);
  const op: UserOperation = {
    callContract: decoded.callContract,
    callData: decoded.callData,
    callGasLimit: "0x" + BigInt(decoded.callGasLimit).toString(16),
    verificationGasLimit:
      "0x" + BigInt(decoded.verificationGasLimit).toString(16),
    maxFeePerGas: "0x" + BigInt(decoded.maxFeePerGas).toString(16),
    maxPriorityFeePerGas:
      "0x" + BigInt(decoded.maxPriorityFeePerGas).toString(16),
    paymasterAndData: decoded.paymasterAndData,
  };
  return op;
}

export function encodeGaslessPayload(op: UserOperation): HexString {
  const data = abiCoder.encodeParameter(USER_OPERATION_ABI_TYPE, op);
  // gasless payload = ENTRYPOINT_HANDLE_OP_SELECTOR + abiEncode(UserOperation)
  return "0x" + ENTRYPOINT_HANDLE_OP_SELECTOR + data.slice(2);
}
