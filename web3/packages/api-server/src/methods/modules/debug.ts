import { Hash } from "@ckb-lumos/base";
import { RPC } from "@ckb-lumos/toolkit";
import { LogItem } from "@godwoken-web3/godwoken";
import { envConfig } from "../../base/env-config";
import { CACHE_EXPIRED_TIME_MILSECS } from "../../cache/constant";
import { Store } from "../../cache/store";
import { ethTxHashToGwTxHash } from "../../cache/tx-hash";
import { Query } from "../../db";
import { parsePolyjuiceUserLog } from "../../filter-web3-tx";
import { POLYJUICE_USER_LOG_FLAG } from "../constant";
import { handleGwError } from "../gw-error";
import { middleware } from "../validator";

export class Debug {
  private readonlyRpc: RPC;
  private cacheStore: Store;
  private query: Query;

  constructor() {
    this.readonlyRpc = new RPC(
      envConfig.godwokenReadonlyJsonRpc || envConfig.godwokenJsonRpc
    );
    this.cacheStore = new Store(true, CACHE_EXPIRED_TIME_MILSECS);
    this.query = new Query();

    this.replayTransaction = middleware(this.replayTransaction.bind(this), 1);
  }

  async replayTransaction(args: any[]) {
    const ethTxHash: Hash = args[0];
    const gwTxHash: Hash | undefined = await ethTxHashToGwTxHash(
      ethTxHash,
      this.query,
      this.cacheStore
    );
    if (gwTxHash == null) {
      throw new Error(`gw tx hash not found by eth tx hash ${ethTxHash}`);
    }
    let result;
    try {
      result = await this.readonlyRpc.debug_replay_transaction(
        gwTxHash,
        ...args.slice(1)
      );
    } catch (error) {
      handleGwError(error);
    }

    if (result == null) {
      return undefined;
    }

    const web3Logs = result.logs
      .filter((log: LogItem) => log.service_flag === POLYJUICE_USER_LOG_FLAG)
      .map((log: LogItem) => {
        const info = parsePolyjuiceUserLog(log.data);
        return {
          address: info.address,
          data: info.data,
          topics: info.topics,
        };
      });

    // Replace logs with web3 logs
    result.logs = web3Logs;

    return result;
  }
}
