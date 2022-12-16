const newrelic = require("newrelic");
import { Hash, HexNumber, HexString, Script } from "@ckb-lumos/base";
import { Reader } from "@ckb-lumos/toolkit";
import { RPC } from "./rpc";
import {
  BlockParameter,
  L2Transaction,
  L2TransactionReceipt,
  L2TransactionWithStatus,
  NodeInfo,
  RawL2Transaction,
  RegistryAddress,
  RunResult,
  U128,
  U32,
} from "./types";
import { SerializeL2Transaction, SerializeRawL2Transaction } from "../schemas";
import {
  NormalizeL2Transaction,
  NormalizeRawL2Transaction,
} from "./normalizers";
import { logger } from "./logger";

export class GodwokenClient {
  private rpc: RPC;
  private readonlyRpc: RPC;

  constructor(url: string, readonlyUrl?: string) {
    this.rpc = new RPC(url);
    this.readonlyRpc = !!readonlyUrl ? new RPC(readonlyUrl) : this.rpc;
  }

  // This RPC only for fullnode
  public async isRequestInQueue(hash: Hash): Promise<boolean> {
    const result = await this.writeRpcCall("is_request_in_queue", hash);
    return result;
  }

  public async getScriptHash(accountId: U32): Promise<Hash> {
    const hash = await this.rpcCall("get_script_hash", toHex(accountId));
    return hash;
  }

  public async getAccountIdByScriptHash(
    scriptHash: Hash
  ): Promise<U32 | undefined> {
    const accountId: HexNumber | undefined = await this.rpcCall(
      "get_account_id_by_script_hash",
      scriptHash
    );
    if (accountId == null) {
      return undefined;
    }
    return +accountId;
  }

  public async getRegistryAddressByScriptHash(
    scriptHash: Hash,
    registryId: U32
  ): Promise<RegistryAddress | undefined> {
    const registryAddress = await this.rpcCall(
      "get_registry_address_by_script_hash",
      scriptHash,
      toHex(registryId)
    );
    return registryAddress;
  }

  public async getScriptHashByRegistryAddress(
    serializedRegistryAddress: HexString
  ): Promise<Hash | undefined> {
    const scriptHash = await this.rpcCall(
      "get_script_hash_by_registry_address",
      serializedRegistryAddress
    );
    return scriptHash;
  }

  public async getBalance(
    serializedRegistryAddress: HexString,
    sudtId: U32,
    blockParameter?: BlockParameter
  ): Promise<U128> {
    const balance: HexNumber = await this.rpcCall(
      "get_balance",
      serializedRegistryAddress,
      toHex(sudtId),
      toHex(blockParameter)
    );
    return BigInt(balance);
  }

  public async getStorageAt(
    accountId: U32,
    key: HexString,
    blockParameter?: BlockParameter
  ): Promise<Hash> {
    return await this.rpcCall(
      "get_storage_at",
      toHex(accountId),
      key,
      toHex(blockParameter)
    );
  }

  public async getScript(scriptHash: Hash): Promise<Script | undefined> {
    return await this.rpcCall("get_script", scriptHash);
  }

  public async getNonce(
    accountId: U32,
    blockParameter?: BlockParameter
  ): Promise<U32> {
    const nonce: HexNumber = await this.writeRpcCall(
      "get_nonce",
      toHex(accountId),
      toHex(blockParameter)
    );
    return +nonce;
  }

  public async getData(
    dataHash: Hash,
    blockParameter?: BlockParameter
  ): Promise<HexString> {
    return await this.rpcCall("get_data", dataHash, toHex(blockParameter));
  }

  // Don't log `invalid exit code 83` error
  public async executeForGetAccountScriptHash(
    rawL2tx: RawL2Transaction,
    blockParameter?: BlockParameter
  ): Promise<RunResult> {
    const data: HexString = new Reader(
      SerializeRawL2Transaction(NormalizeRawL2Transaction(rawL2tx))
    ).serializeJson();
    const name = "gw_execute_raw_l2transaction";
    const result = await this.readonlyRpc[name](data, toHex(blockParameter));
    return result;
  }

  public async executeRawL2Transaction(
    rawL2tx: RawL2Transaction,
    blockParameter?: BlockParameter,
    serializedRegistryAddress?: HexString
  ): Promise<RunResult> {
    const data: HexString = new Reader(
      SerializeRawL2Transaction(NormalizeRawL2Transaction(rawL2tx))
    ).serializeJson();
    const params = [data, toHex(blockParameter)];
    if (serializedRegistryAddress != null) {
      params.push(serializedRegistryAddress);
    }
    return await this.rpcCall("execute_raw_l2transaction", ...params);
  }

  public async executeL2Transaction(l2tx: L2Transaction): Promise<RunResult> {
    const data: HexString = new Reader(
      SerializeL2Transaction(NormalizeL2Transaction(l2tx))
    ).serializeJson();
    return await this.rpcCall("execute_l2transaction", data);
  }

  public async submitL2Transaction(
    l2tx: L2Transaction
  ): Promise<Hash | undefined> {
    const data: HexString = new Reader(
      SerializeL2Transaction(NormalizeL2Transaction(l2tx))
    ).serializeJson();
    return await this.writeRpcCall("submit_l2transaction", data);
  }

  public async getTransaction(
    hash: Hash
  ): Promise<L2TransactionWithStatus | undefined> {
    const txWithStatus = await this.rpcCall("get_transaction", hash);
    if (txWithStatus == null && this.rpc !== this.readonlyRpc) {
      // Only fullnode has queue info
      return await this.writeRpcCall("get_transaction", hash);
    }
    return txWithStatus;
  }

  public async getTransactionReceipt(
    hash: Hash
  ): Promise<L2TransactionReceipt | undefined> {
    return await this.rpcCall("get_transaction_receipt", hash);
  }

  public async getNodeInfo(): Promise<NodeInfo> {
    return await this.rpcCall("get_node_info");
  }

  public async getTipBlockHash(): Promise<HexString> {
    return await this.rpcCall("get_tip_block_hash");
  }

  public async getMemPoolStateRoot(): Promise<HexString> {
    return await this.rpcCall("get_mem_pool_state_root");
  }

  public async ping(): Promise<string> {
    const result = await this.rpcCall("ping");
    return result;
  }

  public async pingFullNode(): Promise<string> {
    const result = await this.writeRpcCall("ping");
    return result;
  }

  public async getBlock(blockHash: HexString): Promise<any> {
    const result = await this.rpcCall("get_block", blockHash);
    return result;
  }

  private async rpcCall(methodName: string, ...args: any[]): Promise<any> {
    const name = "gw_" + methodName;
    try {
      return await newrelic.startSegment(`read_${name}`, true, async () => {
        return this.readonlyRpc[name](...args);
      });
    } catch (err: any) {
      logger.info(`Call gw rpc "${name}" error:`, err.message);
      throw err;
    }
  }

  private async writeRpcCall(methodName: string, ...args: any[]): Promise<any> {
    const name = "gw_" + methodName;
    try {
      return await newrelic.startSegment(`write_${name}`, true, async () => {
        return this.rpc[name](...args);
      });
    } catch (err: any) {
      logger.info(`Call gw rpc "${name}" error:`, err.message);
      throw err;
    }
  }
}

function toHex(num: number | bigint | undefined | null): HexNumber | undefined {
  if (num == null) {
    return undefined;
  }
  return "0x" + num.toString(16);
}
