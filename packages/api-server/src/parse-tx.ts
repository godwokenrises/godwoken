import { HexString, HexNumber, Script, HashType } from "@ckb-lumos/base";
import { Reader } from "@ckb-lumos/toolkit";
import {
  schemas,
  L2Transaction,
  EthAddrRegArgs,
  normalizers,
  EthAddrRegArgsType,
  SetMapping,
  BatchSetMapping,
  EthToGw,
  GwToEth,
  SudtArgs,
  SudtArgsType,
  SudtQuery,
  SudtTransfer,
  MetaContractArgs,
  MetaContractArgsType,
  CreateAccount,
  BatchCreateEthAccounts,
  Fee,
} from "@godwoken-web3/godwoken";
import { Uint64, Uint32, Uint128, Uint256 } from "./base/types/uint";
export interface PolyjuiceArgs {
  isCreate: boolean;
  gasLimit: HexNumber;
  gasPrice: HexNumber;
  value: HexNumber;
  inputSize: HexNumber;
  input: HexString;
  toAddressWhenNativeTransfer: HexString | undefined;
}

export function isPolyjuiceTransactionArgs(polyjuiceArgs: HexString) {
  // header
  const args_0_7 =
    "0x" +
    Buffer.from("FFFFFF", "hex").toString("hex") +
    Buffer.from("POLY", "utf8").toString("hex");

  return polyjuiceArgs.slice(0, 14) !== args_0_7;
}

export function decodePolyjuiceArgs(args: HexString): PolyjuiceArgs {
  if (!isPolyjuiceTransactionArgs(args)) {
    throw new Error("Invalid polyjuice tx args, header not matched!");
  }

  const buf = Buffer.from(args.slice(2), "hex");

  if (buf.byteLength < 52) {
    throw new Error("Tx's length smaller than 52 bytes!");
  }

  const isCreate = buf[7].toString(16) === "3";
  const gasLimit = Uint64.fromLittleEndian(
    "0x" + buf.slice(8, 16).toString("hex")
  ).toHex();
  const gasPrice = Uint128.fromLittleEndian(
    "0x" + buf.slice(16, 32).toString("hex")
  ).toHex();
  const value = Uint128.fromLittleEndian(
    "0x" + buf.slice(32, 48).toString("hex")
  ).toHex();

  const inputSize = Uint32.fromLittleEndian(
    "0x" + buf.slice(48, 52).toString("hex")
  );
  const input = "0x" + buf.slice(52, 52 + inputSize.getValue()).toString("hex");
  let toAddressWhenNativeTransfer: HexString | undefined = undefined;
  if (buf.byteLength === 52 + inputSize.getValue() + 20) {
    toAddressWhenNativeTransfer =
      "0x" +
      buf
        .slice(52 + inputSize.getValue(), 52 + inputSize.getValue() + 20)
        .toString("hex");
  }

  return {
    isCreate,
    gasLimit,
    gasPrice,
    value,
    inputSize: inputSize.toHex(),
    input,
    toAddressWhenNativeTransfer,
  };
}

//******* parser
export function parseSerializeL2Transaction(
  serializedL2Tx: HexString
): L2Transaction {
  const l2tx = new schemas.L2Transaction(new Reader(serializedL2Tx));
  return DenormalizeL2Transaction(l2tx);
}

export function parseSerializeEthAddrRegArgs(args: HexString) {
  return DenormalizeEthAddrRegArgs(
    new schemas.ETHAddrRegArgs(new Reader(args))
  );
}

export function parseSerializeSudtArgs(args: HexString) {
  return DenormalizeSudtArgs(new schemas.SUDTArgs(new Reader(args)));
}

export function parseSerializeMetaContractArgs(args: HexString) {
  return DenormalizeMetaContractArgs(
    new schemas.MetaContractArgs(new Reader(args))
  );
}

//******* serializer
export function serializeL2Transaction(l2Tx: L2Transaction): HexString {
  return new Reader(
    schemas.SerializeL2Transaction(normalizers.NormalizeL2Transaction(l2Tx))
  ).serializeJson();
}

export function serializeSudtArgs(sudtArgs: SudtArgs): HexString {
  return new Reader(
    schemas.SerializeSUDTArgs(normalizers.NormalizeSudtArgs(sudtArgs))
  ).serializeJson();
}

export function serializeEthAddrRegArgs(
  ethAddrRegArgs: EthAddrRegArgs
): HexString {
  return new Reader(
    schemas.SerializeETHAddrRegArgs(
      normalizers.NormalizeEthAddrRegArgs(ethAddrRegArgs)
    )
  ).serializeJson();
}

export function serializeMetaContractArgs(
  metaContractArgs: MetaContractArgs
): HexString {
  return new Reader(
    schemas.SerializeMetaContractArgs(
      normalizers.NormalizeMetaContractArgs(metaContractArgs)
    )
  ).serializeJson();
}

//******* DeNormalizer
export function DenormalizeL2Transaction(l2Tx: schemas.L2Transaction) {
  return {
    raw: DenormalizeRawL2Transaction(l2Tx.getRaw()),
    signature: new Reader(l2Tx.getSignature().raw()).serializeJson(),
  };
}

export function DenormalizeRawL2Transaction(rawL2Tx: schemas.RawL2Transaction) {
  return {
    chain_id: new Uint64(
      rawL2Tx.getChainId().toLittleEndianBigUint64()
    ).toHex(),
    from_id: new Uint32(rawL2Tx.getFromId().toLittleEndianUint32()).toHex(),
    to_id: new Uint32(rawL2Tx.getToId().toLittleEndianUint32()).toHex(),
    nonce: new Uint32(rawL2Tx.getNonce().toLittleEndianUint32()).toHex(),
    args: new Reader(rawL2Tx.getArgs().raw()).serializeJson(),
  };
}

export function DenormalizeSudtArgs(sudtArgs: schemas.SUDTArgs) {
  switch (sudtArgs.unionType()) {
    case SudtArgsType.SUDTQuery:
      return {
        type: sudtArgs.unionType(),
        value: DenormalizeSudtQuery(sudtArgs.value() as schemas.SUDTQuery),
      };

    case SudtArgsType.SUDTTransfer:
      return {
        type: sudtArgs.unionType(),
        value: DenormalizeSudtTransfer(
          sudtArgs.value() as schemas.SUDTTransfer
        ),
      };

    default:
      throw new Error("unsupported type");
  }
}

export function DenormalizeSudtQuery(sudtQuery: schemas.SUDTQuery): SudtQuery {
  const address = new Reader(sudtQuery.getAddress().raw()).serializeJson();

  return {
    address,
  };
}

export function DenormalizeSudtTransfer(
  sudtTransfer: schemas.SUDTTransfer
): SudtTransfer {
  const toAddress = new Reader(
    sudtTransfer.getToAddress().raw()
  ).serializeJson();

  const amount = Uint256.fromLittleEndian(
    new Reader(sudtTransfer.getAmount().raw()).serializeJson()
  ).toHex();

  const feeAmount = Uint128.fromLittleEndian(
    new Reader(sudtTransfer.getFee().getAmount().raw()).serializeJson()
  ).toHex();

  const registryId = new Uint32(
    sudtTransfer.getFee().getRegistryId().toLittleEndianUint32()
  ).toHex();

  return {
    to_address: toAddress,
    amount,
    fee: {
      amount: feeAmount,
      registry_id: registryId,
    },
  };
}

export function DenormalizeEthAddrRegArgs(
  ethAddrRegArgs: schemas.ETHAddrRegArgs
) {
  switch (ethAddrRegArgs.unionType()) {
    case EthAddrRegArgsType.BatchSetMapping:
      return {
        type: ethAddrRegArgs.unionType(),
        value: DenormalizeBatchSetMapping(
          ethAddrRegArgs.value() as schemas.BatchSetMapping
        ),
      };

    case EthAddrRegArgsType.SetMapping:
      return {
        type: ethAddrRegArgs.unionType(),
        value: DenormalizeSetMapping(
          ethAddrRegArgs.value() as schemas.SetMapping
        ),
      };

    case EthAddrRegArgsType.GwToEth:
      return {
        type: ethAddrRegArgs.unionType(),
        value: DenormalizeGwToEth(ethAddrRegArgs.value() as schemas.GwToEth),
      };

    case EthAddrRegArgsType.EthToGw:
      return {
        type: ethAddrRegArgs.unionType(),
        value: DenormalizeEthToGw(ethAddrRegArgs.value() as schemas.EthToGw),
      };

    default:
      throw new Error("unsupported type");
  }
}

export function DenormalizeSetMapping(
  setMapping: schemas.SetMapping
): SetMapping {
  const gwScriptHash = new Reader(
    setMapping.getGwScriptHash().raw()
  ).serializeJson();

  const amount = Uint128.fromLittleEndian(
    new Reader(setMapping.getFee().getAmount().raw()).serializeJson()
  ).toHex();

  const registryId = new Uint32(
    setMapping.getFee().getRegistryId().toLittleEndianUint32()
  ).toHex();

  return {
    gw_script_hash: gwScriptHash,
    fee: {
      registry_id: registryId,
      amount,
    },
  };
}

export function DenormalizeBatchSetMapping(
  batchSetMapping: schemas.BatchSetMapping
): BatchSetMapping {
  const gwScriptHashes = [];
  for (let i = 0; i < batchSetMapping.getGwScriptHashes().length(); i++) {
    const gwScriptHash = new Reader(
      batchSetMapping.getGwScriptHashes().indexAt(i).raw()
    ).serializeJson();

    gwScriptHashes.push(gwScriptHash);
  }

  const amount = Uint128.fromLittleEndian(
    new Reader(batchSetMapping.getFee().getAmount().raw()).serializeJson()
  ).toHex();

  const registryId = new Uint32(
    batchSetMapping.getFee().getRegistryId().toLittleEndianUint32()
  ).toHex();

  return {
    gw_script_hashes: gwScriptHashes,
    fee: {
      registry_id: registryId,
      amount,
    },
  };
}

export function DenormalizeEthToGw(ethToGw: schemas.EthToGw): EthToGw {
  const ethAddress = new Reader(ethToGw.getEthAddress().raw()).serializeJson();
  return {
    eth_address: ethAddress,
  };
}

export function DenormalizeGwToEth(gwToEth: schemas.GwToEth): GwToEth {
  const gwScriptHash = new Reader(
    gwToEth.getGwScriptHash().raw()
  ).serializeJson();
  return {
    gw_script_hash: gwScriptHash,
  };
}

export function DenormalizeMetaContractArgs(
  metaContractArgs: schemas.MetaContractArgs
) {
  switch (metaContractArgs.unionType()) {
    case MetaContractArgsType.CreateAccount:
      return {
        type: metaContractArgs.unionType(),
        value: DenormalizeCreateAccount(
          metaContractArgs.value() as schemas.CreateAccount
        ),
      };

    case MetaContractArgsType.BatchCreateEthAccounts:
      return {
        type: metaContractArgs.unionType(),
        value: DenormalizeBatchCreateEthAccounts(
          metaContractArgs.value() as schemas.BatchCreateEthAccounts
        ),
      };

    default:
      throw new Error("unsupported type");
  }
}

export function DenormalizeCreateAccount(
  createAccount: schemas.CreateAccount
): CreateAccount {
  return {
    script: DenormalizeScript(createAccount.getScript()),
    fee: DenormalizeFee(createAccount.getFee()),
  };
}

export function DenormalizeBatchCreateEthAccounts(
  batchCreateEthAccounts: schemas.BatchCreateEthAccounts
): BatchCreateEthAccounts {
  const scripts: Script[] = [];
  for (let i = 0; i < batchCreateEthAccounts.getScripts().length(); i++) {
    const script = DenormalizeScript(
      batchCreateEthAccounts.getScripts().indexAt(i)
    );
    scripts.push(script);
  }

  return {
    scripts,
    fee: DenormalizeFee(batchCreateEthAccounts.getFee()),
  };
}

export function DenormalizeHashType(hashType: number): HashType {
  switch (hashType) {
    case 0:
      return "data";

    case 1:
      return "type";

    case 2:
      return "data1";

    default:
      throw new Error(`unsupported hash type ${hashType}`);
  }
}

export function DenormalizeScript(script: schemas.Script): Script {
  const args = new Reader(script.getArgs().raw()).serializeJson();
  const code_hash = new Reader(script.getCodeHash().raw()).serializeJson();
  const hash_type = DenormalizeHashType(script.getHashType());
  return {
    code_hash,
    args,
    hash_type,
  };
}

export function DenormalizeFee(fee: schemas.Fee): Fee {
  const feeAmount = Uint128.fromLittleEndian(
    new Reader(fee.getAmount().raw()).serializeJson()
  ).toHex();

  const registryId = new Uint32(
    fee.getRegistryId().toLittleEndianUint32()
  ).toHex();

  return {
    amount: feeAmount,
    registry_id: registryId,
  };
}
