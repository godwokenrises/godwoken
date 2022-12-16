import {
  BlockParameter,
  GodwokenLog,
  LogItem,
  PolyjuiceSystemLog,
  PolyjuiceUserLog,
  SudtOperationLog,
  SudtPayFeeLog,
  TransactionCallObject,
} from "../types";
import { middleware, validators } from "../validator";
import { AutoCreateAccountCacheValue } from "../../cache/types";
import { HexNumber, Hash, Address, HexString, utils } from "@ckb-lumos/base";
import {
  normalizers,
  RunResult,
  schemas,
  GodwokenClient,
} from "@godwoken-web3/godwoken";
import {
  CKB_SUDT_ID,
  POLYJUICE_CONTRACT_CODE,
  POLYJUICE_SYSTEM_LOG_FLAG,
  POLYJUICE_SYSTEM_PREFIX,
  POLYJUICE_USER_LOG_FLAG,
  SUDT_OPERATION_LOG_FLAG,
  SUDT_PAY_FEE_LOG_FLAG,
  AUTO_CREATE_ACCOUNT_FROM_ID,
} from "../constant";
import { Query, universalizeAddress } from "../../db";
import { envConfig } from "../../base/env-config";
import { Uint256, Uint32, Uint64 } from "../../base/types/uint";
import {
  Log,
  toApiBlock,
  toApiLog,
  toApiTransaction,
  toApiTransactionReceipt,
} from "../../db/types";
import {
  HeaderNotFoundError,
  InvalidParamsError,
  isRpcError,
  MethodNotSupportError,
  Web3Error,
} from "../error";
import {
  EthBlock,
  EthLog,
  EthTransaction,
  EthTransactionReceipt,
} from "../../base/types/api";
import { filterWeb3Transaction } from "../../filter-web3-tx";
import { FilterManager } from "../../cache";
import { Store } from "../../cache/store";
import {
  AUTO_CREATE_ACCOUNT_CACHE_EXPIRED_TIME_MILSECS,
  CACHE_EXPIRED_TIME_MILSECS,
  TX_HASH_MAPPING_CACHE_EXPIRED_TIME_MILSECS,
  TX_HASH_MAPPING_PREFIX_KEY,
} from "../../cache/constant";
import {
  autoCreateAccountCacheKey,
  calcEthTxHash,
  ethRawTxToPolyTx,
  ethRawTxToGwTx,
  getSignature,
  polyTxToGwTx,
  polyjuiceRawTransactionToApiTransaction,
  PolyjuiceTransaction,
  ethCallTxToGodwokenRawTx,
} from "../../convert-tx";
import { ethAddressToAccountId, EthRegistryAddress } from "../../base/address";
import { keccakFromString } from "ethereumjs-util";
import { DataCacheConstructor, RedisDataCache } from "../../cache/data";
import { gwConfig, readonlyPriceOracle } from "../../base/index";
import { logger } from "../../base/logger";
import { calcIntrinsicGas } from "../../util";
import { FilterFlag, FilterParams, RpcFilterRequest } from "../../base/filter";
import { Reader } from "@ckb-lumos/toolkit";
import { handleGwError, isGwError } from "../gw-error";
import { ethTxHashToGwTxHash } from "../../cache/tx-hash";
import { EthNormalizer } from "../normalizer";

const Config = require("../../../config/eth.json");

type U32 = number;
type U64 = bigint;

const ZERO_ETH_ADDRESS = "0x" + "00".repeat(20);

type GodwokenBlockParameter = U64 | undefined;

export class Eth {
  private query: Query;
  private rpc: GodwokenClient;
  private instantFinalityHackMode: boolean;
  private filterManager: FilterManager;
  private cacheStore: Store;
  private ethNormalizer: EthNormalizer;

  constructor(instantFinalityHackMode: boolean = false) {
    this.query = new Query();
    this.rpc = new GodwokenClient(
      envConfig.godwokenJsonRpc,
      envConfig.godwokenReadonlyJsonRpc
    );
    this.instantFinalityHackMode = instantFinalityHackMode;
    this.filterManager = new FilterManager(true);
    this.cacheStore = new Store(true, CACHE_EXPIRED_TIME_MILSECS);
    this.ethNormalizer = new EthNormalizer(this.rpc);

    // middleware params validator
    this.getBlockByNumber = middleware(this.getBlockByNumber.bind(this), 2, [
      validators.blockParameter,
      validators.bool,
    ]);
    this.getBlockByHash = middleware(this.getBlockByHash.bind(this), 2, [
      validators.blockHash,
      validators.bool,
    ]);
    this.getBalance = middleware(this.getBalance.bind(this), 2, [
      validators.address,
      validators.blockParameter,
    ]);
    this.getStorageAt = middleware(this.getStorageAt.bind(this), 3, [
      validators.address,
      validators.hexNumber,
      validators.blockParameter,
    ]);
    this.getTransactionCount = middleware(
      this.getTransactionCount.bind(this),
      2,
      [validators.address, validators.blockParameter]
    );
    this.getBlockTransactionCountByHash = middleware(
      this.getBlockTransactionCountByHash.bind(this),
      1,
      [validators.blockHash]
    );
    this.getBlockTransactionCountByNumber = middleware(
      this.getBlockTransactionCountByNumber.bind(this),
      1,
      [validators.blockParameter]
    );
    this.getUncleCountByBlockHash = middleware(
      this.getUncleCountByBlockHash.bind(this),
      1,
      [validators.blockHash]
    );
    this.getUncleCountByBlockNumber = middleware(
      this.getUncleCountByBlockNumber.bind(this),
      1,
      [validators.blockParameter]
    );
    this.getCode = middleware(this.getCode.bind(this), 2, [
      validators.address,
      validators.blockParameter,
    ]);
    this.getTransactionByHash = middleware(
      this.getTransactionByHash.bind(this),
      1,
      [validators.txHash]
    );
    this.getTransactionByBlockHashAndIndex = middleware(
      this.getTransactionByBlockHashAndIndex.bind(this),
      2,
      [validators.blockHash, validators.hexNumber]
    );
    this.getTransactionByBlockNumberAndIndex = middleware(
      this.getTransactionByBlockNumberAndIndex.bind(this),
      2,
      [validators.blockParameter, validators.hexNumber]
    );
    this.getTransactionReceipt = middleware(
      this.getTransactionReceipt.bind(this),
      1,
      [validators.txHash]
    );
    this.getUncleByBlockHashAndIndex = middleware(
      this.getUncleByBlockHashAndIndex.bind(this),
      2,
      [validators.blockHash, validators.hexNumber]
    );
    this.getUncleByBlockNumberAndIndex = middleware(
      this.getUncleByBlockNumberAndIndex.bind(this),
      2,
      [validators.blockParameter, validators.hexNumber]
    );
    this.call = middleware(this.call.bind(this), 2, [
      validators.ethCallParams,
      validators.blockParameter,
    ]);
    this.estimateGas = middleware(this.estimateGas.bind(this), 1, [
      validators.ethEstimateGasParams,
    ]);
    this.newFilter = middleware(this.newFilter.bind(this), 1, [
      validators.newFilterParams,
    ]);
    this.uninstallFilter = middleware(this.uninstallFilter.bind(this), 1, [
      validators.hexString,
    ]);
    this.getFilterLogs = middleware(this.getFilterLogs.bind(this), 1, [
      validators.hexString,
    ]);
    this.getFilterChanges = middleware(this.getFilterChanges.bind(this), 1, [
      validators.hexString,
    ]);
    this.getLogs = middleware(this.getLogs.bind(this), 1, [
      validators.newFilterParams,
    ]);

    this.sendRawTransaction = middleware(
      this.sendRawTransaction.bind(this),
      1,
      [validators.hexString]
    );

    //
    this.syncing = middleware(this.syncing.bind(this), 0);

    this.coinbase = middleware(this.coinbase.bind(this), 0);

    this.mining = middleware(this.mining.bind(this), 0);

    this.blockNumber = middleware(this.blockNumber.bind(this), 0);

    this.sign = middleware(this.sign.bind(this), 0);

    this.signTransaction = middleware(this.signTransaction.bind(this), 0);

    this.sendTransaction = middleware(this.sendTransaction.bind(this), 0);
  }

  chainId(args: []): HexNumber {
    return gwConfig.web3ChainId!;
  }

  /**
   * Returns the current protocol version
   * @param  {Array<*>} [params] An empty array
   * @param  {Function} [cb] A function with an error object as the first argument and the
   * protocol version as the second argument
   */
  protocolVersion(args: []): HexNumber {
    const version = "0x" + BigInt(Config.eth_protocolVersion).toString(16);
    return version;
  }

  /**
   * Returns block syncing info
   * @param  {Array<*>} [params] An empty array
   * @param  {Function} [cb] A function with an error object as the first argument and the
   * SyncingStatus as the second argument.
   *    SyncingStatus: false or { startingBlock, currentBlock, highestBlock }
   */
  async syncing(args: []): Promise<any> {
    // TODO get the latest L2 block number
    const tipNumber = await this.query.getTipBlockNumber();
    if (tipNumber == null) {
      return false;
    }
    const blockHeight: HexNumber = new Uint64(tipNumber).toHex();
    const result = {
      startingBlock: blockHeight,
      currentBlock: blockHeight,
      highestBlock: blockHeight,
    };
    return result;
  }

  /**
   * Returns client coinbase address, which is always zero hashes
   * @param  {Array<*>} [params] An empty array
   * @param  {Function} [cb] A function with an error object as the first argument and the
   * 20 bytes 0 hex string as the second argument.
   */
  coinbase(args: []): Address {
    return ZERO_ETH_ADDRESS;
  }

  /**
   * Returns if client is mining, which is always false
   * @param  {Array<*>} [params] An empty array
   * @param  {Function} [cb] A function with an error object as the first argument and the
   * false as the second argument.
   */
  mining(args: []): boolean {
    return false;
  }

  /**
   * Returns client mining hashrate, which is always 0x0
   * @param  {Array<*>} [params] An empty array
   * @param  {Function} [cb] A function with an error object as the first argument and the
   * 0x0 as the second argument.
   */
  hashrate(args: []): HexNumber {
    return "0x0";
  }

  /**
   * Return median gas_price of latest ${LATEST_MEDIAN_GAS_PRICE} transactions
   *
   * @param _args empty
   * @returns
   */
  async gasPrice(_args: []): Promise<HexNumber> {
    try {
      const gasPrice = await readonlyPriceOracle.gasPrice();
      const gasPriceHex = "0x" + gasPrice.toString(16);
      return gasPriceHex;
    } catch (error: any) {
      throw new Web3Error(error.message);
    }
  }

  /**
   * Returns client saved wallet addresses, which is always zero array
   * @param  {Array<*>} [params] An empty array
   * @param  {Function} [cb] A function with an error object as the first argument and the
   * [] as the second argument.
   */
  accounts(args: []): [] {
    return [];
  }

  async blockNumber(args: []): Promise<HexNumber | null> {
    const tipBlockNumber = await this.query.getTipBlockNumber();
    if (tipBlockNumber == null) {
      return null;
    }
    const blockHeight: HexNumber = new Uint64(tipBlockNumber).toHex();
    return blockHeight;
  }

  async sign(_args: any[]): Promise<void> {
    throw new MethodNotSupportError("eth_sign is not supported!");
  }

  async signTransaction(_args: any[]): Promise<void> {
    throw new MethodNotSupportError("eth_signTransaction is not supported!");
  }

  async sendTransaction(_args: any[]): Promise<void> {
    throw new MethodNotSupportError("eth_sendTransaction is not supported!");
  }

  async getBalance(args: [string, string]): Promise<HexNumber> {
    try {
      const address = args[0];
      const blockParameter = args[1];
      const blockNumber: GodwokenBlockParameter =
        await this.parseBlockParameter(blockParameter);
      const registryAddress: EthRegistryAddress = new EthRegistryAddress(
        address
      );
      const balance = await this.rpc.getBalance(
        registryAddress.serialize(),
        +CKB_SUDT_ID,
        blockNumber
      );

      const balanceHex = new Uint256(balance).toHex();
      return balanceHex;
    } catch (error: any) {
      throw new Web3Error(error.message);
    }
  }

  async getStorageAt(args: [string, string, string]): Promise<HexString> {
    try {
      const address = args[0];
      const storagePosition = args[1];
      const blockParameter = args[2];
      const blockNumber: GodwokenBlockParameter =
        await this.parseBlockParameter(blockParameter);
      const accountId: U32 | undefined = await ethAddressToAccountId(
        address,
        this.rpc
      );
      if (accountId == null) {
        return "0x0000000000000000000000000000000000000000000000000000000000000000";
      }

      const key = buildStorageKey(storagePosition);
      const value = await this.rpc.getStorageAt(accountId, key, blockNumber);
      return value;
    } catch (error: any) {
      throw new Web3Error(error.message);
    }
  }

  /**
   *
   * @param args [address, QUANTITY|TAG]
   * @param callback
   */
  async getTransactionCount(args: [string, string]): Promise<HexNumber> {
    try {
      const address = args[0];
      const blockParameter = args[1];
      const blockNumber: GodwokenBlockParameter =
        await this.parseBlockParameter(blockParameter);
      const accountId: number | undefined = await ethAddressToAccountId(
        address,
        this.rpc
      );
      if (accountId == null) {
        return "0x0";
      }
      const nonce = await this.rpc.getNonce(accountId, blockNumber);
      const transactionCount = new Uint32(nonce).toHex();
      return transactionCount;
    } catch (error: any) {
      throw new Web3Error(error.message);
    }
  }

  async getCode(args: [string, string]): Promise<HexString> {
    try {
      const defaultResult = "0x";

      const address = args[0];
      const blockParameter = args[1];

      if (address === ZERO_ETH_ADDRESS) {
        return defaultResult;
      }

      const blockNumber: GodwokenBlockParameter =
        await this.parseBlockParameter(blockParameter);
      const accountId: number | undefined = await ethAddressToAccountId(
        address,
        this.rpc
      );
      if (accountId == null) {
        return defaultResult;
      }
      const contractCodeKey = polyjuiceBuildContractCodeKey(accountId);
      const dataHash = await this.rpc.getStorageAt(
        accountId,
        contractCodeKey,
        blockNumber
      );
      const data = await this.rpc.getData(dataHash, blockNumber);
      return data || defaultResult;
    } catch (error: any) {
      throw new Web3Error(error.message);
    }
  }

  async call(
    args: [TransactionCallObject, BlockParameter | undefined]
  ): Promise<HexString> {
    try {
      const txCallObj = await this.ethNormalizer.normalizeEstimateGasTx(
        args[0]
      );
      const blockParameter = args[1] || "latest";
      const blockNumber: GodwokenBlockParameter =
        await this.parseBlockParameter(blockParameter);

      const executeCallResult = async () => {
        let runResult: RunResult | undefined = await doEthCall(
          txCallObj,
          this.rpc,
          blockNumber
        );

        logger.debug("RunResult:", runResult);
        return runResult.return_data;
      };

      // using cache
      if (envConfig.enableCacheEthCall === "true") {
        // calculate raw data cache key
        const [tipBlockHash, memPoolStateRoot] = await Promise.all([
          this.rpc.getTipBlockHash(),
          this.rpc.getMemPoolStateRoot(),
        ]);
        const serializeParams = serializeEthCallParameters(
          txCallObj,
          blockNumber
        );
        const rawDataKey = getEthCallCacheKey(
          serializeParams,
          tipBlockHash,
          memPoolStateRoot
        );

        const prefixName = `${this.constructor.name}:call`; // FIXME: ${this.call.name} is null
        const constructArgs: DataCacheConstructor = {
          prefixName,
          rawDataKey,
          executeCallResult,
        };
        const dataCache = new RedisDataCache(constructArgs);
        const return_data = await dataCache.get();
        return return_data;
      } else {
        // not using cache
        const return_data = await executeCallResult();
        return return_data;
      }
    } catch (error: any) {
      if (isGwError(error)) {
        handleGwError(error);
      }
      if (isRpcError(error)) {
        throw error;
      }
      throw new Web3Error(error.message, error.data);
    }
  }

  async estimateGas(
    args: [Partial<TransactionCallObject>, BlockParameter | undefined]
  ): Promise<HexNumber> {
    try {
      const txCallObj = await this.ethNormalizer.normalizeEstimateGasTx(
        args[0]
      );
      const blockParameter = args[1] || "latest";
      const blockNumber: GodwokenBlockParameter =
        await this.parseBlockParameter(blockParameter);

      const extraGas: bigint = BigInt(envConfig.extraEstimateGas || "0");

      const executeCallResult = async () => {
        let runResult: RunResult | undefined = await doEthCall(
          txCallObj,
          this.rpc,
          blockNumber
        );

        const polyjuiceSystemLog = extractPolyjuiceSystemLog(
          runResult.logs
        ) as PolyjuiceSystemLog;

        logger.debug(
          "eth_estimateGas RunResult:",
          runResult,
          "0x" + BigInt(polyjuiceSystemLog.gasUsed).toString(16)
        );

        const gasUsed: bigint = polyjuiceSystemLog.gasUsed + extraGas;

        let result: HexNumber = "0x" + gasUsed.toString(16);
        const intrinsicGas = calcIntrinsicGas(txCallObj.to, txCallObj.data);
        if (gasUsed < intrinsicGas) {
          result = "0x" + intrinsicGas.toString(16);
        }

        return result;
      };

      // using cache
      if (envConfig.enableCacheEstimateGas === "true") {
        // calculate raw data cache key
        const [tipBlockHash, memPoolStateRoot] = await Promise.all([
          this.rpc.getTipBlockHash(),
          this.rpc.getMemPoolStateRoot(),
        ]);
        const serializeParams = serializeEstimateGasParameters(
          txCallObj,
          blockNumber
        );
        const rawDataKey = getEstimateGasCacheKey(
          serializeParams,
          tipBlockHash,
          memPoolStateRoot
        );

        const prefixName = `${this.constructor.name}:estimateGas`; // FIXME: ${this.call.name} is null
        const constructArgs: DataCacheConstructor = {
          prefixName,
          rawDataKey,
          executeCallResult,
        };
        const dataCache = new RedisDataCache(constructArgs);
        const result = await dataCache.get();
        return result;
      } else {
        // not using cache
        const result = await executeCallResult();
        return result;
      }
    } catch (error: any) {
      if (isGwError(error)) {
        handleGwError(error);
      }
      if (isRpcError(error)) {
        throw error;
      }
      throw new Web3Error(error.message);
    }
  }

  async getBlockByHash(args: [string, boolean]): Promise<EthBlock | null> {
    try {
      const blockHash = args[0];
      const isFullTransaction = args[1];

      const block = await this.query.getBlockByHash(blockHash);
      if (block == null) {
        return null;
      }

      if (isFullTransaction) {
        const txs = await this.query.getTransactionsByBlockHash(blockHash);
        const apiTxs = txs.map((tx) => toApiTransaction(tx));
        const apiBlock = toApiBlock(block, apiTxs);
        return apiBlock;
      } else {
        const ethTxHashes: Hash[] =
          await this.query.getTransactionEthHashesByBlockHash(blockHash);
        const apiBlock = toApiBlock(block, ethTxHashes);
        return apiBlock;
      }
    } catch (error: any) {
      if (isRpcError(error)) {
        throw error;
      }
      throw new Web3Error(error.message);
    }
  }

  async getBlockByNumber(args: [string, boolean]): Promise<EthBlock | null> {
    const blockParameter = args[0];
    const isFullTransaction = args[1];
    let blockNumber: U64 | undefined;

    try {
      blockNumber = await this.blockParameterToBlockNumber(blockParameter);
    } catch (error: any) {
      return null;
    }

    const block = await this.query.getBlockByNumber(blockNumber);
    if (block == null) {
      return null;
    }

    const apiBlock = toApiBlock(block);
    if (isFullTransaction) {
      const txs = await this.query.getTransactionsByBlockNumber(blockNumber);
      const apiTxs = txs.map((tx) => toApiTransaction(tx));
      apiBlock.transactions = apiTxs;
    } else {
      const txHashes: Hash[] =
        await this.query.getTransactionEthHashesByBlockNumber(blockNumber);

      apiBlock.transactions = txHashes;
    }
    return apiBlock;
  }

  /**
   *
   * @param args [blockHash]
   * @param callback
   */
  async getBlockTransactionCountByHash(args: [string]): Promise<HexNumber> {
    const blockHash: Hash = args[0];

    const txCount = await this.query.getBlockTransactionCountByHash(blockHash);
    const txCountHex = new Uint32(txCount).toHex();

    return txCountHex;
  }

  /**
   *
   * @param args [blockNumber]
   * @param callback
   */
  async getBlockTransactionCountByNumber(args: [string]): Promise<HexNumber> {
    const blockParameter = args[0];
    const blockNumber: U64 | undefined = await this.blockParameterToBlockNumber(
      blockParameter
    );

    const txCount = await this.query.getBlockTransactionCountByNumber(
      blockNumber
    );
    const txCountHex: HexNumber = new Uint32(txCount).toHex();
    return txCountHex;
  }

  async getUncleByBlockHashAndIndex(args: [string, string]): Promise<null> {
    return null;
  }

  async getUncleByBlockNumberAndIndex(args: [string, string]): Promise<null> {
    return null;
  }

  /**
   *
   * @param args [blockHash]
   * @param callback
   */
  async getUncleCountByBlockHash(args: [string]): Promise<HexNumber> {
    return "0x0";
  }

  /**
   *
   * @param args [blockNumber]
   * @param callback
   */
  async getUncleCountByBlockNumber(args: [string]): Promise<HexNumber> {
    return "0x0";
  }

  /**
   *
   * @param args
   * @returns always empty array
   */
  async getCompilers(args: []): Promise<[]> {
    return [];
  }

  async getTransactionByHash(args: [string]): Promise<EthTransaction | null> {
    const ethTxHash: Hash = args[0];
    const acaCacheKey = autoCreateAccountCacheKey(ethTxHash);

    // 1. Find in db
    const tx = await this.query.getTransactionByEthTxHash(ethTxHash);
    if (tx != null) {
      // no need await
      // delete auto create account tx if already in db
      this.cacheStore.delete(acaCacheKey);
      const apiTx = toApiTransaction(tx);
      return apiTx;
    }

    // 2. Find pending tx from gw mempool block
    const ethTxHashKey = ethTxHashCacheKey(ethTxHash);
    const gwTxHash: Hash | null = await this.cacheStore.get(ethTxHashKey);
    if (gwTxHash != null) {
      const godwokenTxWithStatus = await this.rpc.getTransaction(gwTxHash);
      if (godwokenTxWithStatus == null) {
        return null;
      }
      const godwokenTxReceipt = await this.rpc.getTransactionReceipt(gwTxHash);
      const tipBlock = await this.query.getTipBlock();
      if (tipBlock == null) {
        throw new Error("tip block not found!");
      }
      let ethTxInfo = undefined;
      try {
        ethTxInfo = await filterWeb3Transaction(
          ethTxHash,
          this.rpc,
          tipBlock.number,
          tipBlock.hash,
          godwokenTxWithStatus.transaction,
          godwokenTxReceipt
        );
      } catch (err) {
        logger.error("filterWeb3Transaction:", err);
        logger.info("godwoken tx:", godwokenTxWithStatus);
        logger.info("godwoken receipt:", godwokenTxReceipt);
        throw err;
      }
      if (ethTxInfo != null) {
        const ethTx = ethTxInfo[0];
        return ethTx;
      }
    }

    // 3. Find by auto create account tx
    // TODO: delete cache store if dropped by godwoken
    // convert to tx hash mapping store if account id generated ?
    const polyjuiceRawTx = await this.cacheStore.get(acaCacheKey);
    if (polyjuiceRawTx != null) {
      const tipBlock = await this.query.getTipBlock();
      if (tipBlock == null) {
        throw new Error("tip block not found!");
      }
      // Convert polyjuice tx to api transaction
      const { tx, fromAddress }: AutoCreateAccountCacheValue =
        JSON.parse(polyjuiceRawTx);
      const isAcaTxExist: boolean = await this.isAcaTxExist(
        ethTxHash,
        tx,
        fromAddress
      );
      logger.info(
        `aca tx: action: getTransactionByHash, eth_tx_hash: ${ethTxHash}, is_tx_exist: ${isAcaTxExist}`
      );
      if (isAcaTxExist) {
        const apiTransaction: EthTransaction =
          polyjuiceRawTransactionToApiTransaction(
            tx,
            ethTxHash,
            tipBlock.hash,
            tipBlock.number,
            fromAddress
          );
        return apiTransaction;
      } else {
        // If not found, means dropped by godwoken, should delete cache
        this.cacheStore.delete(acaCacheKey);
      }
    }

    return null;
  }

  /**
   *
   * @param args [blockHash, index]
   * @param callback
   */
  async getTransactionByBlockHashAndIndex(
    args: [string, string]
  ): Promise<EthTransaction | null> {
    const blockHash: Hash = args[0];
    const index = +args[1];

    const tx = await this.query.getTransactionByBlockHashAndIndex(
      blockHash,
      index
    );
    if (tx == null) {
      return null;
    }
    const apiTx = toApiTransaction(tx);
    return apiTx;
  }

  async getTransactionByBlockNumberAndIndex(
    args: [string, string]
  ): Promise<EthTransaction | null> {
    const blockParameter = args[0];
    const index: U32 = +args[1];
    const blockNumber: U64 = await this.blockParameterToBlockNumber(
      blockParameter
    );

    const tx = await this.query.getTransactionByBlockNumberAndIndex(
      blockNumber,
      index
    );

    if (tx == null) {
      return null;
    }

    const apiTx = toApiTransaction(tx);
    return apiTx;
  }

  async getTransactionReceipt(
    args: [string]
  ): Promise<EthTransactionReceipt | null> {
    const ethTxHash: Hash = args[0];
    const gwTxHash: Hash | undefined = await ethTxHashToGwTxHash(
      ethTxHash,
      this.query,
      this.cacheStore
    );
    if (gwTxHash == null) {
      return null;
    }

    // 1. Find in db
    const data = await this.query.getTransactionAndLogsByHash(gwTxHash);
    if (data != null) {
      const [tx, logs] = data;
      const apiLogs = logs.map((log) => toApiLog(log, ethTxHash));
      const transactionReceipt = toApiTransactionReceipt(tx, apiLogs);
      return transactionReceipt;
    }

    // 2. If under instant-finality hack mode, build receipt from gw mempool block
    if (this.instantFinalityHackMode) {
      logger.debug(
        `[eth_getTransactionReceipt] find ${ethTxHash} with instant-finality hack`
      );
      const godwokenTxWithStatus = await this.rpc.getTransaction(gwTxHash);
      if (godwokenTxWithStatus == null) {
        return null;
      }
      const godwokenTxReceipt = await this.rpc.getTransactionReceipt(gwTxHash);
      if (godwokenTxReceipt == null) {
        return null;
      }
      const tipBlock = await this.query.getTipBlock();
      if (tipBlock == null) {
        throw new Error(`tip block not found`);
      }
      let ethTxInfo = undefined;
      try {
        ethTxInfo = await filterWeb3Transaction(
          ethTxHash,
          this.rpc,
          tipBlock.number,
          tipBlock.hash,
          godwokenTxWithStatus.transaction,
          godwokenTxReceipt
        );
      } catch (err) {
        logger.error("filterWeb3Transaction:", err);
        logger.info("godwoken tx:", godwokenTxWithStatus);
        logger.info("godwoken receipt:", godwokenTxReceipt);
        throw err;
      }
      if (ethTxInfo != null) {
        const ethTxReceipt = ethTxInfo[1]!;
        return ethTxReceipt;
      }
    }

    return null;
  }

  /* #region filter-related api methods */
  async newFilter(args: [RpcFilterRequest]): Promise<HexString> {
    const tipLog: Log | null = await this.query.getTipLog();
    const initialLogId: bigint = tipLog == null ? 0n : tipLog.id;
    const filter_id = await this.filterManager.install(args[0], initialLogId);
    return filter_id;
  }

  async newBlockFilter(args: []): Promise<HexString> {
    const tipBlockNum = await this.getTipNumber();
    const filter_id = await this.filterManager.install(
      FilterFlag.blockFilter,
      tipBlockNum
    );
    return filter_id;
  }

  async newPendingTransactionFilter(args: []): Promise<HexString> {
    const tipBlockNum = await this.getTipNumber();
    const filter_id = await this.filterManager.install(
      FilterFlag.pendingTransaction,
      tipBlockNum
    );
    return filter_id;
  }

  async uninstallFilter(args: [HexString]): Promise<boolean> {
    const filter_id = args[0];
    const isUninstalled = await this.filterManager.uninstall(filter_id);
    return isUninstalled;
  }

  /**
   * This method only works for filters creates with `eth_newFilter` not for filters created using `eth_newBlockFilter`
   * or `eth_newPendingTransactionFilter`, which will return empty array.
   *
   * @returns {(Log|Array)} array of log objects, or an empty array if nothing has changed since last poll.
   *
   * @throws {Web3Error} - filter not found
   */
  async getFilterLogs(args: [string]): Promise<Array<any>> {
    const filter_id = args[0];
    const filter = await this.filterManager.get(filter_id);

    if (filter == null) {
      throw new Web3Error("filter not found");
    } else if (
      filter === FilterFlag.blockFilter ||
      filter === FilterFlag.pendingTransaction
    ) {
      return [];
    } else {
      return await this.getFilterChanges(args);
    }
  }

  /**
   * Polling method for a filter, which returns an array of events that have occurred since the last poll.
   *
   * @returns {array} - Array of one of the following, depending on the filter type, or empty if no changes since last poll:
   * - `eth_newBlockFilter`
   *   `blockHash` - The 32 byte hash of a block that meets your filter requirements, asc order by block number
   * - `eth_newPendingTransactionFilter`
   *   `[]` - Godwoken-Web3 doesn't support `eth_newPendingTransactionFilter` yet.
   * - `eth_newFilter`
   *   - `logindex` - Integer of log index position in the block encoded as a hexadecimal.
   *   - `transactionindex` - Integer of transaction index position log was created from.
   *   - `transactionhash` - Hash of the transactions this log was created from.
   *   - `blockhash` - Hash of the block where this log was in.
   *   - `blocknumber` - The block number where this log was, encoded as a hexadecimal.
   *   - `address` - The address from which this log originated.
   *   - `data` - Contains one or more 32 Bytes non-indexed arguments of the log.
   *   - `topics` - Array of 0 to 4 32 Bytes of indexed log arguments.
   *
   * @throws {Web3Error} - filter not found
   */
  async getFilterChanges(args: [string]): Promise<Hash[] | EthLog[]> {
    const filter_id = args[0];
    const filter = await this.filterManager.get(filter_id);
    if (!filter) {
      throw new Web3Error("filter not found");
    } else if (filter === FilterFlag.blockFilter) {
      const lastPollBlockNumber = await this.filterManager.getLastPoll(
        filter_id
      );
      const arrayOfHashAndNumber =
        await this.query.getBlockHashesAndNumbersAfterBlockNumber(
          lastPollBlockNumber,
          "asc"
        );
      if (arrayOfHashAndNumber.length !== 0) {
        await this.filterManager.updateLastPoll(
          filter_id,
          arrayOfHashAndNumber[arrayOfHashAndNumber.length - 1].number
        );
      }
      return arrayOfHashAndNumber.map((hn) => hn.hash);
    } else if (filter === FilterFlag.pendingTransaction) {
      return [];
    } else {
      const lastPollLogId = await this.filterManager.getLastPoll(filter_id);
      const logs = await this.query.getLogsByFilter(
        await this._rpcFilterRequestToGetLogsParams(filter),
        lastPollLogId
      );

      if (logs.length !== 0) {
        await this.filterManager.updateLastPoll(
          filter_id,
          logs[logs.length - 1].id
        );
      }

      return logs.map((log) => toApiLog(log, log.eth_tx_hash!));
    }
  }

  async getLogs(args: [RpcFilterRequest]): Promise<EthLog[]> {
    return await this._getLogs(
      await this._rpcFilterRequestToGetLogsParams(args[0])
    );
  }

  async _getLogs(filter: FilterParams): Promise<EthLog[]> {
    const logs = await this.query.getLogsByFilter(filter);
    return logs.map((log) => toApiLog(log, log.eth_tx_hash!));
  }

  /* #endregion */

  // return gw tx hash
  async sendRawTransaction(args: [string]): Promise<Hash> {
    try {
      const data = args[0];
      const [rawTx, autoCreateCacheKeyAndValue] = await ethRawTxToGwTx(
        data,
        this.rpc
      );
      const gwTxHash = await this.rpc.submitL2Transaction(rawTx);
      logger.info("eth_sendRawTransaction gw hash:", gwTxHash);
      // cache auto create account tx if submit success
      if (autoCreateCacheKeyAndValue != null) {
        await this.cacheStore.insert(
          autoCreateCacheKeyAndValue[0],
          autoCreateCacheKeyAndValue[1],
          AUTO_CREATE_ACCOUNT_CACHE_EXPIRED_TIME_MILSECS
        );
      }
      const ethTxHash = calcEthTxHash(data);
      logger.info("eth_sendRawTransaction eth hash:", ethTxHash);

      // save the tx hash mapping for instant finality
      if (gwTxHash != null) {
        await this.cacheTxHashMapping(ethTxHash, gwTxHash);
      }

      return ethTxHash;
    } catch (error: any) {
      logger.error("failed to sendRawTransaction, error: ", error);
      if (isGwError(error)) {
        handleGwError(error);
      }
      throw new InvalidParamsError(error.message);
    }
  }

  private async cacheTxHashMapping(ethTxHash: Hash, gwTxHash: Hash) {
    const ethTxHashKey = ethTxHashCacheKey(ethTxHash);
    await this.cacheStore.insert(
      ethTxHashKey,
      gwTxHash,
      TX_HASH_MAPPING_CACHE_EXPIRED_TIME_MILSECS
    );
    const gwTxHashKey = gwTxHashCacheKey(gwTxHash);
    await this.cacheStore.insert(
      gwTxHashKey,
      ethTxHash,
      TX_HASH_MAPPING_CACHE_EXPIRED_TIME_MILSECS
    );
  }

  private async getTipNumber(): Promise<U64> {
    const num = await this.query.getTipBlockNumber();
    if (num == null) {
      throw new Error("tip block number not found!!");
    }
    return num;
  }

  private async parseBlockParameter(
    blockParameter: BlockParameter
  ): Promise<GodwokenBlockParameter> {
    switch (blockParameter) {
      case "latest":
        if (this.instantFinalityHackMode) {
          // under instant-finality hack, we treat latest as pending
          return undefined;
        }
        return await this.getTipNumber();
      case "earliest":
        return 0n;
      // It's supposed to be filtered in the validator, so throw an error if matched
      case "pending":
        // null means pending in godwoken
        return undefined;
    }

    // handle block hash
    // Note: don't check requireCanonical since Godwoken only has canonical blocks.
    if (
      typeof blockParameter === "object" &&
      blockParameter.blockHash != null
    ) {
      const block = await this.getBlockByHash([
        blockParameter.blockHash,
        false,
      ]);
      if (block == null) {
        throw new HeaderNotFoundError(
          `Header not found by block hash ${blockParameter.blockHash}`
        );
      }
      if (block.number == null) {
        // means pending;
        return undefined;
      }
      return BigInt(block.number);
    }

    // handle block number
    const tipNumber: bigint = await this.getTipNumber();
    const blockHexNum =
      typeof blockParameter === "object" && blockParameter.blockNumber != null
        ? blockParameter.blockNumber
        : (blockParameter as HexNumber);
    const blockNumber: U64 = Uint64.fromHex(blockHexNum).getValue();
    if (tipNumber < blockNumber) {
      throw new HeaderNotFoundError();
    }
    return blockNumber;
  }

  // Some RPCs does not support pending parameter
  // eth_getBlockByNumber/eth_getBlockTransactionCountByNumber/eth_getTransactionByBlockNumberAndIndex
  // TODO: maybe we should support for those as well?
  private async blockParameterToBlockNumber(
    blockParameter: BlockParameter
  ): Promise<U64> {
    const blockNumber: GodwokenBlockParameter = await this.parseBlockParameter(
      blockParameter
    );
    if (blockNumber === undefined) {
      return await this.getTipNumber();
    }
    return blockNumber;
  }

  private async _rpcFilterRequestToGetLogsParams(
    filter: RpcFilterRequest
  ): Promise<FilterParams> {
    if (filter.blockHash != null) {
      if (filter.fromBlock !== undefined || filter.toBlock !== undefined) {
        throw new Web3Error(
          "blockHash is mutually exclusive with fromBlock/toBlock"
        );
      }

      const block = await this.query.getBlockByHash(filter.blockHash);
      if (block == null) {
        throw new InvalidParamsError("blockHash cannot be found");
      }

      filter.fromBlock = "0x" + block.number.toString(16);
      filter.toBlock = "0x" + block.number.toString(16);
    }

    const [fromBlock, toBlock] =
      await this._normalizeBlockParameterForFilterRequest(
        filter.fromBlock,
        filter.toBlock
      );
    return {
      fromBlock,
      toBlock,
      topics: filter.topics || [],
      addresses: universalizeAddress(filter.address),
      blockHash: filter.blockHash,
    };
  }

  private async _normalizeBlockParameterForFilterRequest(
    fromBlock: undefined | BlockParameter,
    toBlock: undefined | BlockParameter
  ): Promise<[bigint, bigint]> {
    let normalizedFromBlock: bigint;
    let normalizedToBlock: bigint;
    const latestBlockNumber = await this.getTipNumber();
    // See also:
    // - https://github.com/nervosnetwork/godwoken-web3/pull/427#discussion_r918904239
    // - https://github.com/nervosnetwork/godwoken-web3/pull/300/files/131542bd5cc272279d27760e258fb5fa5de6fc9a#r861541728
    const _fromBlock: bigint | undefined = await this.parseBlockParameter(
      fromBlock ?? "earliest"
    );
    normalizedFromBlock = _fromBlock ?? latestBlockNumber;

    const _toBlock: bigint | undefined = await this.parseBlockParameter(
      toBlock ?? "latest"
    );
    normalizedToBlock = _toBlock ?? latestBlockNumber;

    return [normalizedFromBlock, normalizedToBlock];
  }

  // aca = auto create account
  // `acaTx` is the first transaction (nonce=0) of an undeposited account which account_id/from_id is not undetermined yet.
  // `signature_hash` is used here to get an `acaTx` from GodwokenRPC, see also:
  // https://github.com/nervosnetwork/godwoken/blob/develop/docs/RPC.md#method-gw_submit_l2transaction
  //
  // `gw_get_transaction(signature_hash)`
  //       |-> if `txWithStatus.transaction` != null
  //             |-> found!
  //       |-> if `txWithStatus.transaction` == null
  //             |-> if `from_id` == null
  //                   |-> not found!
  //             |-> if `from_id` != null
  //                   |-> `gw_get_transaction(gw_tx_hash)`
  //                         |-> `txWithStatus.transaction` != null
  //                               |-> found!
  //                         |->  `txWithStatus.transaction` == null
  //                               |-> not found!
  private async isAcaTxExist(
    ethTxHash: Hash,
    rawTx: HexString,
    fromAddress: HexString
  ): Promise<boolean> {
    const tx: PolyjuiceTransaction = ethRawTxToPolyTx(rawTx);
    const signature: HexString = getSignature(tx);
    const signatureHash: Hash = utils
      .ckbHash(new Reader(signature).toArrayBuffer())
      .serializeJson();
    const txWithStatus = await this.rpc.getTransaction(signatureHash);
    if (txWithStatus != null) {
      logger.debug(
        `aca tx: ${ethTxHash} found by signature hash: ${signatureHash}`
      );
      // transaction found by signature hash
      return true;
    }

    const fromId = await ethAddressToAccountId(fromAddress, this.rpc);
    logger.debug(`aca tx's (${ethTxHash}) from_id:`, fromId);
    if (fromId == null) {
      return false;
    }
    const [godwokenTx, _cacheKeyAndValue] = await polyTxToGwTx(
      tx,
      this.rpc,
      rawTx
    );
    if (godwokenTx.raw.from_id === AUTO_CREATE_ACCOUNT_FROM_ID) {
      logger.warn("aca generated tx's from_id = 0");
      return false;
    }
    const gwTxHash: Hash = utils
      .ckbHash(
        new Reader(
          schemas.SerializeRawL2Transaction(
            normalizers.NormalizeRawL2Transaction(godwokenTx.raw)
          )
        ).toArrayBuffer()
      )
      .serializeJson();
    logger.debug(`aca tx: ${ethTxHash} gw_tx_hash: ${gwTxHash}`);
    const gwTx = await this.rpc.getTransaction(gwTxHash);

    return !!gwTx;
  }
}

function ethTxHashCacheKey(ethTxHash: string) {
  return `${TX_HASH_MAPPING_PREFIX_KEY}:eth:${ethTxHash}`;
}

function gwTxHashCacheKey(gwTxHash: string) {
  return `${TX_HASH_MAPPING_PREFIX_KEY}:gw:${gwTxHash}`;
}

function polyjuiceBuildContractCodeKey(accountId: number) {
  return polyjuiceBuildSystemKey(accountId, POLYJUICE_CONTRACT_CODE);
}

function polyjuiceBuildSystemKey(accountId: number, fieldType: number) {
  let key = new Uint8Array(32);
  const array = uint32ToLeBytes(accountId) as number[];
  key[0] = array[0];
  key[1] = array[1];
  key[2] = array[2];
  key[3] = array[3];
  key[4] = POLYJUICE_SYSTEM_PREFIX;
  key[5] = fieldType;
  return "0x" + Buffer.from(key).toString("hex");
}

// function ethStoragePositionToRawKey(ethStoragePosition: string) {}

function uint32ToLeBytes(id: number) {
  let hex = id.toString(16);
  if (hex.length < 8) {
    hex = "0".repeat(8 - hex.length) + hex;
  }
  const array = hex
    .match(/../g)
    ?.reverse()
    .map((x) => {
      return parseInt("0x" + x);
    });
  return array;
}

function buildStorageKey(storagePosition: string) {
  let key = storagePosition.slice(2);
  // If b is larger than len(h), b will be cropped from the left.
  if (key.length > 64) {
    key = key.slice(0, 64);
  }
  if (key.length < 64) {
    key = "0".repeat(64 - key.length) + key;
  }
  logger.debug("storage position:", key);
  return "0x" + key;
}

async function doEthCall(
  tx: Required<TransactionCallObject>,
  rpc: GodwokenClient,
  blockNumber?: U64
): Promise<RunResult> {
  const [rawL2Transaction, serializedRegistryAddress] =
    await ethCallTxToGodwokenRawTx(tx, rpc);
  const runResult = await rpc.executeRawL2Transaction(
    rawL2Transaction,
    blockNumber,
    serializedRegistryAddress
  );

  return runResult;
}

function extractPolyjuiceSystemLog(logItems: LogItem[]): GodwokenLog {
  for (const logItem of logItems) {
    if (logItem.service_flag === "0x2") {
      return parseLog(logItem);
    }
  }
  throw new Error(
    `Can't found PolyjuiceSystemLog, logItems: ${JSON.stringify(logItems)}`
  );
}

// https://github.com/nervosnetwork/godwoken-polyjuice/blob/v0.6.0-rc1/polyjuice-tests/src/helper.rs#L122
function parseLog(logItem: LogItem): GodwokenLog {
  switch (logItem.service_flag) {
    case SUDT_OPERATION_LOG_FLAG:
      return parseSudtOperationLog(logItem);
    case SUDT_PAY_FEE_LOG_FLAG:
      return parseSudtPayFeeLog(logItem);
    case POLYJUICE_SYSTEM_LOG_FLAG:
      return parsePolyjuiceSystemLog(logItem);
    case POLYJUICE_USER_LOG_FLAG:
      return parsePolyjuiceUserLog(logItem);
    default:
      throw new Error(`Can't parse logItem: ${logItem}`);
  }
}
function parseSudtOperationLog(logItem: LogItem): SudtOperationLog {
  let buf = Buffer.from(logItem.data.slice(2), "hex");
  if (buf.length !== 4 + 4 + 16) {
    throw new Error(
      `invalid sudt operation log raw data length: ${buf.length}`
    );
  }
  const fromId = buf.readUInt32LE(0);
  const toId = buf.readUInt32LE(4);
  const amount = buf.readBigUInt64LE(8);
  return {
    sudtId: +logItem.account_id,
    fromId: fromId,
    toId: toId,
    amount: amount,
  };
}

function parseSudtPayFeeLog(logItem: LogItem): SudtPayFeeLog {
  let buf = Buffer.from(logItem.data.slice(2), "hex");
  if (buf.length !== 4 + 4 + 16) {
    throw new Error(
      `invalid sudt operation log raw data length: ${buf.length}`
    );
  }
  const fromId = buf.readUInt32LE(0);
  const blockProducerId = buf.readUInt32LE(4);
  const amount = buf.readBigUInt64LE(8);
  return {
    sudtId: +logItem.account_id,
    fromId: fromId,
    blockProducerId: blockProducerId,
    amount: amount,
  };
}

function parsePolyjuiceSystemLog(logItem: LogItem): PolyjuiceSystemLog {
  let buf = Buffer.from(logItem.data.slice(2), "hex");
  if (buf.length !== 8 + 8 + 16 + 4 + 4) {
    throw new Error(`invalid system log raw data length: ${buf.length}`);
  }
  const gasUsed = buf.readBigUInt64LE(0);
  const cumulativeGasUsed = buf.readBigUInt64LE(8);
  const createdAddress = "0x" + buf.slice(16, 32).toString("hex");
  const statusCode = buf.readUInt32LE(32);
  return {
    gasUsed: gasUsed,
    cumulativeGasUsed: cumulativeGasUsed,
    createdAddress: createdAddress,
    statusCode: statusCode,
  };
}

function parsePolyjuiceUserLog(logItem: LogItem): PolyjuiceUserLog {
  const buf = Buffer.from(logItem.data.slice(2), "hex");
  let offset = 0;
  const address = buf.slice(offset, offset + 20);
  offset += 20;
  const dataSize = buf.readUInt32LE(offset);
  offset += 4;
  const logData = buf.slice(offset, offset + dataSize);
  offset += dataSize;
  const topics_count = buf.readUInt32LE(offset);
  offset += 4;
  let topics = [];
  for (let i = 0; i < topics_count; i++) {
    const topic = buf.slice(offset, offset + 32);
    offset += 32;
    topics.push("0x" + topic.toString("hex"));
  }

  if (offset !== buf.length) {
    throw new Error(
      `Too many bytes for polyjuice user log data: offset=${offset}, data.len()=${buf.length}`
    );
  }

  return {
    address: "0x" + address.toString("hex"),
    data: "0x" + logData.toString("hex"),
    topics: topics,
  };
}

function serializeEthCallParameters(
  ethCallObj: TransactionCallObject,
  blockNumber?: GodwokenBlockParameter
): HexString {
  // since we have check enough balance in eth_call, we need to add gasPrice in cache key
  const toSerializeObj = {
    from: ethCallObj.from || "0x",
    to: ethCallObj.to,
    gas: ethCallObj.gas || "0x",
    gasPrice: ethCallObj.gasPrice || "0x",
    data: ethCallObj.data || "0x",
    value: ethCallObj.value || "0x",
    blockNumber: blockNumber ? "0x" + blockNumber?.toString(16) : "0x", // undefined means pending block, the key contains tipBlockHash, so there is no need to diff pending height
  };
  return JSON.stringify(toSerializeObj);
}

function getEthCallCacheKey(
  serializeEthCallParams: string,
  tipBlockHash: HexString,
  memPoolStateRoot: HexString
) {
  const hash = "0x" + keccakFromString(serializeEthCallParams).toString("hex");
  const id = `0x${tipBlockHash.slice(2, 18)}${memPoolStateRoot.slice(
    2,
    18
  )}${hash.slice(2, 18)}`;
  return id;
}

function serializeEstimateGasParameters(
  estimateGasObj: Partial<TransactionCallObject>,
  blockNumber?: GodwokenBlockParameter
): HexString {
  // since we have check enough balance in eth_call, we need to add gasPrice in cache key
  const toSerializeObj = {
    from: estimateGasObj.from || "0x",
    to: estimateGasObj.to || "0x",
    gas: estimateGasObj.gas || "0x",
    gasPrice: estimateGasObj.gasPrice || "0x",
    data: estimateGasObj.data || "0x",
    value: estimateGasObj.value || "0x",
    blockNumber: blockNumber ? "0x" + blockNumber?.toString(16) : "0x", // undefined means pending block, the key contains tipBlockHash, so there is no need to diff pending height
  };
  return JSON.stringify(toSerializeObj);
}

function getEstimateGasCacheKey(
  serializeEstimateGasParams: string,
  tipBlockHash: HexString,
  memPoolStateRoot: HexString
) {
  const hash =
    "0x" + keccakFromString(serializeEstimateGasParams).toString("hex");
  const id = `0x${tipBlockHash.slice(2, 18)}${memPoolStateRoot.slice(
    2,
    18
  )}${hash.slice(2, 18)}`;
  return id;
}
