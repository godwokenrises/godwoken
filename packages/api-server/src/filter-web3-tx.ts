import { HexString } from "@ckb-lumos/base";
import { Script } from "@ckb-lumos/base";
import { Hash, HexNumber } from "@ckb-lumos/base";
import {
  HexU32,
  U32,
  L2Transaction,
  L2TransactionReceipt,
  GodwokenClient,
  schemas,
  U64,
} from "@godwoken-web3/godwoken";
import { Reader } from "@ckb-lumos/toolkit";
import { EthTransaction, EthTransactionReceipt } from "./base/types/api";
import { Uint128, Uint256, Uint32, Uint64 } from "./base/types/uint";
import { PolyjuiceSystemLog, PolyjuiceUserLog } from "./base/types/gw-log";
import {
  CKB_SUDT_ID,
  ZERO_ETH_ADDRESS,
  DEFAULT_LOGS_BLOOM,
  POLYJUICE_SYSTEM_LOG_FLAG,
  POLYJUICE_USER_LOG_FLAG,
} from "./methods/constant";
import { gwConfig } from "./base/index";
import { logger } from "./base/logger";
import { EthRegistryAddress } from "./base/address";
import { decodePolyjuiceArgs } from "./parse-tx";
import { getRealV } from "./db";

export const PENDING_TRANSACTION_INDEX = "0x0";

export async function filterWeb3Transaction(
  ethTxHash: Hash,
  rpc: GodwokenClient,
  tipBlockNumber: U64,
  tipBlockHash: Hash,
  l2Tx: L2Transaction,
  l2TxReceipt?: L2TransactionReceipt
): Promise<[EthTransaction, EthTransactionReceipt | undefined] | undefined> {
  const pendingBlockHash = bumpHash(tipBlockHash);
  const pendingBlockNumber = new Uint64(tipBlockNumber + 1n).toHex();

  const fromId: U32 = +l2Tx.raw.from_id;
  const fromScriptHash: Hash | undefined = await rpc.getScriptHash(fromId);
  if (fromScriptHash == null) {
    return undefined;
  }
  const fromScript: Script | undefined = await rpc.getScript(fromScriptHash);
  if (fromScript == null) {
    return undefined;
  }

  // skip tx with non eth_account_lock from_id
  if (fromScript.code_hash !== gwConfig.eoaScripts.eth.typeHash) {
    return undefined;
  }

  const fromScriptArgs: HexString = fromScript.args;
  if (
    fromScriptArgs.length !== 106 ||
    fromScriptArgs.slice(0, 66) !== gwConfig.rollupCell.typeHash
  ) {
    logger.error("Wrong from_address's script args:", fromScriptArgs);
    return undefined;
  }

  const fromAddress: HexString = "0x" + fromScriptArgs.slice(66);

  const toId: U32 = +l2Tx.raw.to_id;
  const toScriptHash: Hash | undefined = await rpc.getScriptHash(toId);
  if (toScriptHash == null) {
    return undefined;
  }
  const toScript: Script | undefined = await rpc.getScript(toScriptHash);
  if (toScript == null) {
    return undefined;
  }

  const signature: HexString = l2Tx.signature;
  // 0..32 bytes
  let r = "0x" + signature.slice(2, 66);
  // Remove r left zeros
  r = "0x" + BigInt(r).toString(16);
  // 32..64 bytes
  let s = "0x" + signature.slice(66, 130);
  // Remove s left zeros
  s = "0x" + BigInt(s).toString(16);
  // signature[65] byte
  const signatureV = Uint32.fromHex("0x" + signature.slice(130, 132));
  const chainId = l2Tx.raw.chain_id;
  const v = new Uint128(
    getRealV(BigInt(signatureV.getValue()), BigInt(chainId))
  );

  const nonce: HexU32 = l2Tx.raw.nonce;

  if (
    toScript.code_hash === gwConfig.backends.polyjuice.validatorScriptTypeHash
  ) {
    const l2TxArgs: HexNumber = l2Tx.raw.args;
    const polyjuiceArgs = decodePolyjuiceArgs(l2TxArgs);

    // For CREATE contracts, tx.to_address = null;
    // for native transfers, tx.to_address = last 20 bytes of polyjuice_args;
    // otherwise, tx.to_address equals to the eth_address of tx.to_id.
    let toAddress: HexString | undefined;

    // let polyjuiceChainId: HexNumber | undefined;
    if (polyjuiceArgs.isCreate) {
      toAddress = undefined;
      // polyjuiceChainId = toIdHex;
    } else if (polyjuiceArgs.toAddressWhenNativeTransfer != null) {
      toAddress = polyjuiceArgs.toAddressWhenNativeTransfer;
    } else {
      // 74 = 2 + (32 + 4) * 2
      toAddress = "0x" + toScript.args.slice(74);
      // 32..36 bytes
      // const data = "0x" + toScript.args.slice(66, 74);
      // polyjuiceChainId = "0x" + readUInt32LE(data).toString(16);
    }
    // const chainId = polyjuiceChainId;
    const input = polyjuiceArgs.input || "0x";

    const ethTx: EthTransaction = {
      blockHash: l2TxReceipt ? pendingBlockHash : null,
      blockNumber: l2TxReceipt ? pendingBlockNumber : null,
      transactionIndex: l2TxReceipt ? PENDING_TRANSACTION_INDEX : null,
      from: fromAddress,
      gas: polyjuiceArgs.gasLimit,
      gasPrice: polyjuiceArgs.gasPrice,
      hash: ethTxHash,
      input,
      nonce,
      to: toAddress || null,
      value: polyjuiceArgs.value,
      v: v.toHex(),
      r,
      s,
    };

    if (l2TxReceipt == null) {
      return [ethTx, undefined];
    }

    // receipt info
    const polyjuiceSystemLog = l2TxReceipt.logs.find(
      (log) => log.service_flag === POLYJUICE_SYSTEM_LOG_FLAG
    );
    if (polyjuiceSystemLog == null) {
      throw new Error("No system log found!");
    }
    const logInfo = parsePolyjuiceSystemLog(polyjuiceSystemLog.data);

    let contractAddress = undefined;
    if (polyjuiceArgs.isCreate && logInfo.createdAddress !== ZERO_ETH_ADDRESS) {
      contractAddress = logInfo.createdAddress;
    }

    const txGasUsed = logInfo.gasUsed;
    // or cumulativeGasUsed ?
    const cumulativeGasUsed = txGasUsed;

    const web3Logs = l2TxReceipt.logs
      .filter((log) => log.service_flag === POLYJUICE_USER_LOG_FLAG)
      .map((log, index) => {
        const info = parsePolyjuiceUserLog(log.data);
        return {
          address: info.address,
          data: info.data,
          logIndex: new Uint32(index).toHex(),
          topics: info.topics,
        };
      });

    const receipt: EthTransactionReceipt = {
      transactionHash: ethTxHash,
      transactionIndex: PENDING_TRANSACTION_INDEX,
      blockHash: pendingBlockHash,
      blockNumber: pendingBlockNumber,
      from: fromAddress,
      to: toAddress || null,
      gasUsed: txGasUsed,
      cumulativeGasUsed: cumulativeGasUsed,
      logsBloom: DEFAULT_LOGS_BLOOM,
      logs: web3Logs.map((log) => {
        return {
          ...log,
          data: log.data === "0x" ? "0x" + "00".repeat(32) : log.data,
          blockHash: pendingBlockHash,
          blockNumber: pendingBlockNumber,
          transactionIndex: PENDING_TRANSACTION_INDEX,
          transactionHash: ethTxHash,
          removed: false,
        };
      }),
      contractAddress: contractAddress || null,
      status: l2TxReceipt.exit_code === "0x0" ? "0x1" : "0x0",
    };

    return [ethTx, receipt];
  } else if (
    toId === +CKB_SUDT_ID &&
    toScript.code_hash === gwConfig.gwScripts.l2Sudt.typeHash
  ) {
    const sudtArgs = new schemas.SUDTArgs(new Reader(l2Tx.raw.args));
    if (sudtArgs.unionType() === "SUDTTransfer") {
      const sudtTransfer: schemas.SUDTTransfer = sudtArgs.value();
      const toAddressRegistryAddress = new Reader(
        sudtTransfer.getToAddress().raw()
      ).serializeJson();
      const toAddress = EthRegistryAddress.Deserialize(
        toAddressRegistryAddress
      ).address;
      if (toAddress.length !== 42) {
        return undefined;
      }
      const amount = Uint256.fromLittleEndian(
        new Reader(sudtTransfer.getAmount().raw()).serializeJson()
      );
      const fee = Uint128.fromLittleEndian(
        new Reader(sudtTransfer.getFee().getAmount().raw()).serializeJson()
      );
      const value: Uint256 = amount;
      const gasPrice: Uint128 = new Uint128(1n);
      const gasLimit: Uint128 = fee;

      const ethTx: EthTransaction = {
        blockHash: l2TxReceipt ? pendingBlockHash : null,
        blockNumber: l2TxReceipt ? pendingBlockNumber : null,
        transactionIndex: l2TxReceipt ? PENDING_TRANSACTION_INDEX : null,
        from: fromAddress,
        gas: gasLimit.toHex(),
        gasPrice: gasPrice.toHex(),
        hash: ethTxHash,
        input: "0x",
        nonce,
        to: toAddress,
        value: value.toHex(),
        v: v.toHex(),
        r,
        s,
      };

      const receipt: EthTransactionReceipt = {
        transactionHash: ethTxHash,
        transactionIndex: PENDING_TRANSACTION_INDEX,
        blockHash: pendingBlockHash,
        blockNumber: pendingBlockNumber,
        from: fromAddress,
        to: toAddress,
        gasUsed: gasLimit.toHex(),
        cumulativeGasUsed: gasLimit.toHex(),
        logsBloom: DEFAULT_LOGS_BLOOM,
        logs: [],
        contractAddress: null,
        status: "0x1",
      };

      return [ethTx, receipt];
    }
  }

  return undefined;
}

export function parsePolyjuiceSystemLog(data: HexString): PolyjuiceSystemLog {
  // 2 + (8 + 8 + 20 + 4) * 2
  if (data.length !== 82) {
    throw new Error(`invalid system log raw data length: ${data.length}`);
  }

  const dataWithoutPrefix = data.slice(2);

  // 0..8 bytes
  const gasUsed: Uint64 = Uint64.fromLittleEndian(
    "0x" + dataWithoutPrefix.slice(0, 16)
  );
  // 8..16 bytes
  const cumulativeGasUsed: Uint64 = Uint64.fromLittleEndian(
    "0x" + dataWithoutPrefix.slice(16, 32)
  );
  // 16..36 bytes
  const createdAddress = "0x" + dataWithoutPrefix.slice(32, 72);
  // 36..40 bytes
  const statusCode = Uint32.fromLittleEndian(
    "0x" + dataWithoutPrefix.slice(72, 80)
  ).toHex();

  return {
    gasUsed: gasUsed.toHex(),
    cumulativeGasUsed: cumulativeGasUsed.toHex(),
    createdAddress,
    statusCode,
  };
}

export function parsePolyjuiceUserLog(data: HexString): PolyjuiceUserLog {
  const dataWithoutPrefix = data.slice(2);

  let offset = 0;
  // 0..20 bytes
  const address = "0x" + dataWithoutPrefix.slice(offset, offset + 40);
  offset += 40;
  const dataSize: U32 = Uint32.fromLittleEndian(
    "0x" + dataWithoutPrefix.slice(offset, offset + 8)
  ).getValue();
  offset += 8;
  const logData = "0x" + dataWithoutPrefix.slice(offset, offset + dataSize * 2);
  offset += dataSize * 2;

  const topicsCount: U32 = Uint32.fromLittleEndian(
    "0x" + dataWithoutPrefix.slice(offset, offset + 8)
  ).getValue();
  offset += 8;
  const topics: Hash[] = [];
  for (let i = 0; i < topicsCount; i++) {
    const topic = "0x" + dataWithoutPrefix.slice(offset, offset + 64);
    offset += 64;
    topics.push(topic);
  }
  if (offset !== dataWithoutPrefix.length) {
    throw new Error(
      `Too many bytes for polyjuice user log data: offset=${
        offset / 2
      }, data.length=${dataWithoutPrefix.length / 2}`
    );
  }
  return {
    address,
    data: logData,
    topics,
  };
}

export function bumpHash(hash: Hash): Hash {
  const hashNum = BigInt(hash) + 1n;
  return "0x" + hashNum.toString(16).padStart(64, "0");
}
