import { HexNumber, HexString } from "@ckb-lumos/base";
import { rlp } from "ethereumjs-util";

export interface PolyjuiceTransaction {
  nonce: HexNumber;
  gasPrice: HexNumber;
  gasLimit: HexNumber;
  to: HexString;
  value: HexNumber;
  data: HexString;
  v: HexNumber;
  r: HexString;
  s: HexString;
}

export function toRlpNumber(num: HexNumber): bigint {
  return num === "0x" ? 0n : BigInt(num);
}

export function decodeEthRawTx(ethRawTx: HexString): PolyjuiceTransaction {
  const result: Buffer[] = rlp.decode(ethRawTx) as Buffer[];
  if (result.length !== 9) {
    throw new Error("decode eth raw transaction data error");
  }

  // todo: r might be "0x" which cause inconvenient for down-stream
  const resultHex = result.map((r) => "0x" + Buffer.from(r).toString("hex"));
  const [nonce, gasPrice, gasLimit, to, value, data, v, r, s] = resultHex;
  return {
    nonce,
    gasPrice,
    gasLimit,
    to,
    value,
    data,
    v,
    r,
    s,
  };
}

export function encodePolyjuiceTransaction(tx: PolyjuiceTransaction) {
  const { nonce, gasPrice, gasLimit, to, value, data, v, r, s } = tx;

  const beforeEncode = [
    toRlpNumber(nonce),
    toRlpNumber(gasPrice),
    toRlpNumber(gasLimit),
    to,
    toRlpNumber(value),
    data,
    toRlpNumber(v),
    toRlpNumber(r),
    toRlpNumber(s),
  ];

  const result = rlp.encode(beforeEncode);
  return "0x" + result.toString("hex");
}
