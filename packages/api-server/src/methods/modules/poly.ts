import { Hash, HexNumber } from "@ckb-lumos/base";
import { envConfig } from "../../base/env-config";
import { MethodNotSupportError, Web3Error } from "../error";
import { GodwokenClient } from "@godwoken-web3/godwoken";
import { gwConfig, readonlyPriceOracle } from "../../base/index";
import { Store } from "../../cache/store";
import { CACHE_EXPIRED_TIME_MILSECS } from "../../cache/constant";
import { Query } from "../../db";
import { ethTxHashToGwTxHash, gwTxHashToEthTxHash } from "../../cache/tx-hash";
import { middleware, validators } from "../validator";
import { MAX_ALLOW_SYNC_BLOCKS_DIFF } from "../constant";
import { globalClient } from "../../cache/redis";
const { version: web3Version } = require("../../../package.json");

export class Poly {
  private rpc: GodwokenClient;
  private cacheStore: Store;
  private query: Query;

  constructor() {
    this.rpc = new GodwokenClient(
      envConfig.godwokenJsonRpc,
      envConfig.godwokenReadonlyJsonRpc
    );
    this.cacheStore = new Store(true, CACHE_EXPIRED_TIME_MILSECS);
    this.query = new Query();

    this.getGwTxHashByEthTxHash = middleware(
      this.getGwTxHashByEthTxHash.bind(this),
      1,
      [validators.txHash]
    );
    this.getEthTxHashByGwTxHash = middleware(
      this.getEthTxHashByGwTxHash.bind(this),
      1,
      [validators.txHash]
    );
  }

  async getCreatorId(_args: []): Promise<HexNumber> {
    try {
      const creatorIdHex = gwConfig.accounts.polyjuiceCreator.id;
      return creatorIdHex;
    } catch (err: any) {
      throw new Web3Error(err.message);
    }
  }

  // from in eth_call is optional, DEFAULT_FROM_ADDRESS fills it when empty
  async getDefaultFromId(_args: []): Promise<HexNumber> {
    return gwConfig.accounts.defaultFrom.id;
  }

  async getContractValidatorTypeHash(_args: []): Promise<Hash> {
    return gwConfig.backends.polyjuice.validatorScriptTypeHash;
  }

  async getRollupTypeHash(_args: []): Promise<Hash> {
    return gwConfig.rollupCell.typeHash;
  }

  async getRollupConfigHash(_args: []): Promise<Hash> {
    throw new MethodNotSupportError("ROLLUP_CONFIG_HASH not supported!");
  }

  async getEthAccountLockHash(_args: []): Promise<Hash> {
    return gwConfig.eoaScripts.eth.typeHash;
  }

  async getChainInfo(_args: []): Promise<any> {
    throw new MethodNotSupportError(
      "getChainInfo is deprecated! please use poly_version"
    );
  }

  async version() {
    return {
      versions: {
        web3Version,
        web3IndexerVersion: web3Version, // indexer and api-server should use the same version
        godwokenVersion: gwConfig.nodeVersion,
      },
      nodeInfo: {
        nodeMode: gwConfig.nodeMode,
        rollupCell: gwConfig.rollupCell,
        rollupConfig: gwConfig.rollupConfig,
        gwScripts: gwConfig.gwScripts,
        eoaScripts: gwConfig.eoaScripts,
        backends: gwConfig.backends,
        accounts: gwConfig.accounts,
        chainId: gwConfig.web3ChainId,
        gaslessTx: {
          support: !!gwConfig.entrypointContract,
          entrypointAddress: gwConfig.entrypointContract?.address,
        },
      },
    };
  }

  async getGwTxHashByEthTxHash(args: [Hash]): Promise<Hash | undefined> {
    const ethTxHash = args[0];
    return await ethTxHashToGwTxHash(ethTxHash, this.query, this.cacheStore);
  }

  async getEthTxHashByGwTxHash(args: [Hash]): Promise<Hash | undefined> {
    const gwTxHash = args[0];
    return await gwTxHashToEthTxHash(gwTxHash, this.query, this.cacheStore);
  }

  async getHealthStatus(_args: []) {
    const [
      pingNode,
      pingFullNode,
      pingRedis,
      isDBConnected,
      syncBlocksDiff,
      ckbOraclePrice,
    ] = await Promise.all([
      this.rpc.ping(),
      this.rpc.pingFullNode(),
      globalClient.PING(),
      this.query.isConnected(),
      this.syncBlocksDiff(),
      envConfig.enablePriceOracle == "true"
        ? readonlyPriceOracle.price()
        : "PriceOracleNotEnabled",
    ]);

    const status =
      pingNode === "pong" &&
      pingFullNode === "pong" &&
      pingRedis === "PONG" &&
      isDBConnected &&
      syncBlocksDiff <= MAX_ALLOW_SYNC_BLOCKS_DIFF &&
      ckbOraclePrice != null;

    return {
      status,
      pingNode,
      pingFullNode,
      pingRedis,
      isDBConnected,
      syncBlocksDiff,
      ckbOraclePrice,
    };
  }

  // get block data sync status
  private async syncBlocksDiff(): Promise<Number> {
    const blockNum = await this.query.getTipBlockNumber();
    if (blockNum == null) {
      throw new Error("db tipBlockNumber is null");
    }
    const dbTipBlockNumber = +blockNum.toString();

    const tipBlockHash = await this.rpc.getTipBlockHash();
    const tipBlock = await this.rpc.getBlock(tipBlockHash);
    const gwTipBlockNumber = +tipBlock.block.raw.number;

    const diff = gwTipBlockNumber - dbTipBlockNumber;
    return diff;
  }
}
