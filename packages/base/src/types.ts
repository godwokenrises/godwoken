import { Hash, HexNumber, Script } from "@ckb-lumos/base";
import { normalizers, BigIntToHexString, Reader } from "ckb-js-toolkit";

// Taken for now from https://github.com/xxuejie/ckb-js-toolkit/blob/68f5ff709f78eb188ee116b2887a362123b016cc/src/normalizers.js#L17-L69,
// later we can think about exposing those functions directly.
function normalizeHexNumber(length: number) {
  return function (debugPath: string, value: any) {
    if (!(value instanceof ArrayBuffer)) {
      let intValue = BigInt(value).toString(16);
      if (intValue.length % 2 !== 0) {
        intValue = "0" + intValue;
      }
      if (intValue.length / 2 > length) {
        throw new Error(
          `${debugPath} is ${
            intValue.length / 2
          } bytes long, expected length is ${length}!`
        );
      }
      const view = new DataView(new ArrayBuffer(length));
      for (let i = 0; i < intValue.length / 2; i++) {
        const start = intValue.length - (i + 1) * 2;
        view.setUint8(i, parseInt(intValue.substr(start, 2), 16));
      }
      value = view.buffer;
    }
    if (value.byteLength < length) {
      const array = new Uint8Array(length);
      array.set(new Uint8Array(value), 0);
      value = array.buffer;
    }
    return value;
  };
}

function normalizeRawData(length: number) {
  return function (debugPath: string, value: any) {
    value = new Reader(value).toArrayBuffer();
    if (length > 0 && value.byteLength !== length) {
      throw new Error(
        `${debugPath} has invalid length ${value.byteLength}, required: ${length}`
      );
    }
    return value;
  };
}

function normalizeObject(debugPath: string, obj: any, keys: object) {
  const result: any = {};

  for (const [key, f] of Object.entries(keys)) {
    const value = obj[key];
    if (!value) {
      throw new Error(`${debugPath} is missing ${key}!`);
    }
    result[key] = f(`${debugPath}.${key}`, value);
  }
  return result;
}

function toNormalize(normalize: Function) {
  return function (debugPath: string, value: any) {
    return normalize(value, {
      debugPath,
    });
  };
}

export interface DepositionRequest {
  capacity: HexNumber;
  amount: HexNumber;
  sudtScript: Script;
  script: Script;
}

export function NormalizeDepositionRequest(
  request: object,
  { debugPath = "deposition_request" } = {}
) {
  return normalizeObject(debugPath, request, {
    capacity: normalizeHexNumber(8),
    amount: normalizeHexNumber(16),
    sudtScript: toNormalize(normalizers.NormalizeScript),
    script: toNormalize(normalizers.NormalizeScript),
  });
}

export interface HeaderInfo {
  number: HexNumber;
  block_hash: Hash;
}

export function NormalizeHeaderInfo(
  headerInfo: object,
  { debugPath = "header_info" } = {}
) {
  return normalizeObject(debugPath, headerInfo, {
    number: normalizeHexNumber(8),
    block_hash: normalizeRawData(32),
  });
}

export interface CustodianLockArgs {
  owner_lock_hash: Hash;
  deposition_block_hash: Hash;
  deposition_block_number: HexNumber;
}

export function NormalizeCustodianLockArgs(
  args: object,
  { debugPath = "custondian_lock_args" } = {}
) {
  return normalizeObject(debugPath, args, {
    owner_lock_hash: normalizeRawData(32),
    deposition_block_hash: normalizeRawData(32),
    deposition_block_number: normalizeHexNumber(8),
  });
}
