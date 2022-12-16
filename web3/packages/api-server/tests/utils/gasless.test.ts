import test from "ava";
import {
  decodeGaslessPayload,
  encodeGaslessPayload,
  UserOperation,
} from "../../src/gasless/payload";

test("encode gasless payload", (t) => {
  const userOperation: UserOperation = {
    callContract: "0x1dF923E4F009663B0Fddc1775dac783B85f432fB",
    callData: "0xffff",
    callGasLimit: "0x61a8",
    verificationGasLimit: "0x61a8",
    maxFeePerGas: "0x61a8",
    maxPriorityFeePerGas: "0x61a8",
    paymasterAndData: "0x1df923e4f009663b0fddc1775dac783b85f432fb",
  };

  const payload = encodeGaslessPayload(userOperation);
  t.deepEqual(
    payload,
    "0xfb4350d800000000000000000000000000000000000000000000000000000000000000200000000000000000000000001df923e4f009663b0fddc1775dac783b85f432fb00000000000000000000000000000000000000000000000000000000000000e000000000000000000000000000000000000000000000000000000000000061a800000000000000000000000000000000000000000000000000000000000061a800000000000000000000000000000000000000000000000000000000000061a800000000000000000000000000000000000000000000000000000000000061a800000000000000000000000000000000000000000000000000000000000001200000000000000000000000000000000000000000000000000000000000000002ffff00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000141df923e4f009663b0fddc1775dac783b85f432fb000000000000000000000000"
  );
});

test("decode gasless payload", (t) => {
  const payload =
    "0xfb4350d800000000000000000000000000000000000000000000000000000000000000200000000000000000000000001df923e4f009663b0fddc1775dac783b85f432fb00000000000000000000000000000000000000000000000000000000000000e000000000000000000000000000000000000000000000000000000000000061a800000000000000000000000000000000000000000000000000000000000061a800000000000000000000000000000000000000000000000000000000000061a800000000000000000000000000000000000000000000000000000000000061a800000000000000000000000000000000000000000000000000000000000001200000000000000000000000000000000000000000000000000000000000000002ffff00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000141df923e4f009663b0fddc1775dac783b85f432fb000000000000000000000000";
  const userOperation = decodeGaslessPayload(payload);
  t.deepEqual(userOperation, {
    callContract: "0x1dF923E4F009663B0Fddc1775dac783B85f432fB",
    callData: "0xffff",
    callGasLimit: "0x61a8",
    verificationGasLimit: "0x61a8",
    maxFeePerGas: "0x61a8",
    maxPriorityFeePerGas: "0x61a8",
    paymasterAndData: "0x1df923e4f009663b0fddc1775dac783b85f432fb",
  });
});