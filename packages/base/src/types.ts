import { Hash, HexNumber, Script, denormalizers } from "@ckb-lumos/base";
import { normalizers, BigIntToHexString, Reader } from "ckb-js-toolkit";
import * as schemas from "../schemas/godwoken";

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
  sudt_script_hash: Hash;
  script: Script;
}

export function NormalizeDepositionRequest(
  request: object,
  { debugPath = "deposition_request" } = {}
) {
  return normalizeObject(debugPath, request, {
    capacity: normalizeHexNumber(8),
    amount: normalizeHexNumber(16),
    sudt_script_hash: normalizeRawData(32),
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

export interface DepositionLockArgs {
  owner_lock_hash: Hash;
  layer2_lock: Script;
  cancel_timeout: HexNumber;
}

export function DenormalizeDepositionLockArgs(
  lockArgs: schemas.DepositionLockArgs
) {
  return {
    owner_lock_hash: new Reader(
      lockArgs.getOwnerLockHash().raw()
    ).serializeJson(),
    layer2_lock: denormalizers.DenormalizeScript(lockArgs.getLayer2Lock()),
    cancel_timeout:
      "0x" + lockArgs.getCancelTimeout().toLittleEndianBigUint64().toString(16),
  };
}

export function DenormalizeRawWithdrawalRequest(
  rawWithdrawalRequest: schemas.RawWithdrawalRequest
) {
  return {
    nonce:
      "0x" +
      rawWithdrawalRequest.getNonce().toLittleEndianUint32().toString(16),
    capacity:
      "0x" +
      rawWithdrawalRequest.getCapacity().toLittleEndianBigUint64().toString(16),
    amount: new Reader(rawWithdrawalRequest.getAmount().raw()).serializeJson(),
    sudt_script_hash: new Reader(
      rawWithdrawalRequest.getSudtScriptHash().raw()
    ).serializeJson(),
    account_script_hash: new Reader(
      rawWithdrawalRequest.getAccountScriptHash().raw()
    ).serializeJson(),
    sell_amount: new Reader(
      rawWithdrawalRequest.getSellAmount().raw()
    ).serializeJson(),
    sell_capacity:
      "0x" +
      rawWithdrawalRequest
        .getSellCapacity()
        .toLittleEndianBigUint64()
        .toString(16),
    owner_lock_hash: new Reader(
      rawWithdrawalRequest.getOwnerLockHash().raw()
    ).serializeJson(),
    payment_lock_hash: new Reader(
      rawWithdrawalRequest.getPaymentLockHash().raw()
    ).serializeJson(),
  };
}

export function NormalizeDepositionLockArgs(
  depositionLockArgs: object,
  { debugPath = "deposition_lock_args" } = {}
) {
  return normalizeObject(debugPath, depositionLockArgs, {
    owner_lock_hash: normalizeRawData(32),
    layer2_lock: toNormalize(normalizers.NormalizeScript),
    cancel_timeout: normalizeHexNumber(8),
  });
}

export interface CustodianLockArgs {
  deposition_lock_args: DepositionLockArgs;
  deposition_block_hash: Hash;
  deposition_block_number: HexNumber;
}

export function NormalizeCustodianLockArgs(
  args: object,
  { debugPath = "custondian_lock_args" } = {}
) {
  return normalizeObject(debugPath, args, {
    deposition_lock_args: toNormalize(NormalizeDepositionLockArgs),
    deposition_block_hash: normalizeRawData(32),
    deposition_block_number: normalizeHexNumber(8),
  });
}

export function NormalizeWithdrawalLockArgs(
  args: object,
  { debugPath = "withdrawal_lock_args" } = {}
) {
  return normalizeObject(debugPath, args, {
    deposition_block_hash: normalizeRawData(32),
    deposition_block_number: normalizeHexNumber(8),
    withdrawal_block_hash: normalizeRawData(32),
    withdrawal_block_number: normalizeHexNumber(8),
    sudt_script_hash: normalizeRawData(32),
    sell_capacity: normalizeHexNumber(8),
    sell_amount: normalizeHexNumber(16),
    owner_lock_hash: normalizeRawData(32),
    payment_lock_hash: normalizeRawData(32),
  });
}
