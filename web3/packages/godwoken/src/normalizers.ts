import { Reader, normalizers } from "@ckb-lumos/toolkit";
import {
  EthAddrRegArgs,
  EthAddrRegArgsType,
  EthToGw,
  Fee,
  BatchSetMapping,
  SetMapping,
  GwToEth,
  L2Transaction,
  RawL2Transaction,
  SudtArgsType,
  SudtArgs,
  SudtTransfer,
  SudtQuery,
  MetaContractArgs,
  MetaContractArgsType,
  CreateAccount,
  BatchCreateEthAccounts,
} from "./types";

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

function toNormalizeArray(normalizeFunction: Function) {
  return function (debugPath: string, array: any[]) {
    return array.map((item, i) => {
      return normalizeFunction(`${debugPath}[${i}]`, item);
    });
  };
}

export function NormalizeFee(fee: Fee, { debugPath = "fee" } = {}) {
  return normalizeObject(debugPath, fee, {
    registry_id: normalizeHexNumber(4),
    amount: normalizeHexNumber(16),
  });
}

export function NormalizeSetMapping(
  setMapping: SetMapping,
  { debugPath = "set_mapping" } = {}
) {
  return normalizeObject(debugPath, setMapping, {
    gw_script_hash: normalizeRawData(32),
    fee: toNormalize(NormalizeFee),
  });
}

export function NormalizeBatchSetMapping(
  batchSetMapping: BatchSetMapping,
  { debugPath = "batch_set_mapping" } = {}
) {
  return normalizeObject(debugPath, batchSetMapping, {
    gw_script_hashes: toNormalizeArray(normalizeRawData(32)),
    fee: toNormalize(NormalizeFee),
  });
}

export function NormalizeEthToGw(
  ethToGw: EthToGw,
  { debugPath = "eth_to_gw" } = {}
) {
  return normalizeObject(debugPath, ethToGw, {
    eth_address: normalizeRawData(20),
  });
}

export function NormalizeGwToEth(
  gwToEth: GwToEth,
  { debugPath = "gw_to_eth" } = {}
) {
  return normalizeObject(debugPath, gwToEth, {
    gw_script_hash: normalizeRawData(32),
  });
}

function NormalizeEthAddrRegArgsType() {
  return function (debugPath: string, value: any) {
    if (Object.values(EthAddrRegArgsType).includes(value)) {
      return value;
    }

    throw new Error(`${debugPath} Unsupported type ${value}`);
  };
}

export function NormalizeEthAddrRegArgs(
  ethAddrRegArgs: EthAddrRegArgs,
  { debugPath = "eth_addr_reg_args" } = {}
) {
  switch (ethAddrRegArgs.type) {
    case EthAddrRegArgsType.SetMapping:
      return normalizeObject(debugPath, ethAddrRegArgs, {
        type: NormalizeEthAddrRegArgsType(),
        value: toNormalize(NormalizeSetMapping),
      });

    case EthAddrRegArgsType.BatchSetMapping:
      return normalizeObject(debugPath, ethAddrRegArgs, {
        type: NormalizeEthAddrRegArgsType(),
        value: toNormalize(NormalizeBatchSetMapping),
      });

    default:
      throw new Error(
        `normalizer for ${ethAddrRegArgs.type} is not supported yet`
      );
  }
}

/** SUDT */
function NormalizeSudtArgsType() {
  return function (debugPath: string, value: any) {
    if (Object.values(SudtArgsType).includes(value)) {
      return value;
    }

    throw new Error(`${debugPath} Unsupported type ${value}`);
  };
}

export function NormalizeSudtQuery(
  sudtQuery: SudtQuery,
  { debugPath = "sudt_transfer" } = {}
) {
  return normalizeObject(debugPath, sudtQuery, {
    address: normalizeRawData(-1),
  });
}

export function NormalizeSudtTransfer(
  sudtTransfer: SudtTransfer,
  { debugPath = "sudt_transfer" } = {}
) {
  return normalizeObject(debugPath, sudtTransfer, {
    to_address: normalizeRawData(-1),
    amount: normalizeHexNumber(32),
    fee: toNormalize(NormalizeFee),
  });
}

export function NormalizeSudtArgs(
  sudtArgs: SudtArgs,
  { debugPath = "eth_addr_reg_args" } = {}
) {
  switch (sudtArgs.type) {
    case SudtArgsType.SUDTQuery:
      return normalizeObject(debugPath, sudtArgs, {
        type: NormalizeSudtArgsType(),
        value: toNormalize(NormalizeSudtQuery),
      });

    case SudtArgsType.SUDTTransfer:
      return normalizeObject(debugPath, sudtArgs, {
        type: NormalizeSudtArgsType(),
        value: toNormalize(NormalizeSudtTransfer),
      });

    default:
      throw new Error(`normalizer for ${sudtArgs.type} is not supported yet`);
  }
}

/** MetaContract */
export function NormalizeCreateAccount(
  createAccount: CreateAccount,
  { debugPath = "create_account" } = {}
) {
  return normalizeObject(debugPath, createAccount, {
    script: toNormalize(normalizers.NormalizeScript),
    fee: toNormalize(NormalizeFee),
  });
}

export function NormalizeBatchCreateEthAccounts(
  batchCreateEthAccounts: BatchCreateEthAccounts,
  { debugPath = "batch_create_eth_accounts" } = {}
) {
  return normalizeObject(debugPath, batchCreateEthAccounts, {
    scripts: toNormalizeArray(toNormalize(normalizers.NormalizeScript)),
    fee: toNormalize(NormalizeFee),
  });
}

function NormalizeMetaContractArgsType() {
  return function (debugPath: string, value: any) {
    if (Object.values(MetaContractArgsType).includes(value)) {
      return value;
    }

    throw new Error(`${debugPath} Unsupported type ${value}`);
  };
}

export function NormalizeMetaContractArgs(
  metaContractArgs: MetaContractArgs,
  { debugPath = "meta_contract_args" } = {}
) {
  switch (metaContractArgs.type) {
    case MetaContractArgsType.CreateAccount:
      return normalizeObject(debugPath, metaContractArgs, {
        type: NormalizeMetaContractArgsType(),
        value: toNormalize(NormalizeCreateAccount),
      });

    case MetaContractArgsType.BatchCreateEthAccounts:
      return normalizeObject(debugPath, metaContractArgs, {
        type: NormalizeMetaContractArgsType(),
        value: toNormalize(NormalizeBatchCreateEthAccounts),
      });

    default:
      throw new Error(
        `normalizer for ${metaContractArgs.type} is not supported yet`
      );
  }
}

export function NormalizeRawL2Transaction(
  rawL2Transaction: RawL2Transaction,
  { debugPath = "raw_l2_transaction" } = {}
) {
  return normalizeObject(debugPath, rawL2Transaction, {
    chain_id: normalizeHexNumber(8),
    from_id: normalizeHexNumber(4),
    to_id: normalizeHexNumber(4),
    nonce: normalizeHexNumber(4),
    args: normalizeRawData(-1),
  });
}

export function NormalizeL2Transaction(
  l2Transaction: L2Transaction,
  { debugPath = "l2_transaction" } = {}
) {
  return normalizeObject(debugPath, l2Transaction, {
    raw: toNormalize(NormalizeRawL2Transaction),
    signature: normalizeRawData(65),
  });
}
