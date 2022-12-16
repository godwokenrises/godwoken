import { Hash, HexString } from "@ckb-lumos/base";
import { Query } from "../db";
import {
  TX_HASH_MAPPING_CACHE_EXPIRED_TIME_MILSECS,
  TX_HASH_MAPPING_PREFIX_KEY,
} from "./constant";
import { Store } from "./store";

function ethTxHashCacheKey(ethTxHash: string) {
  return `${TX_HASH_MAPPING_PREFIX_KEY}:eth:${ethTxHash}`;
}

function gwTxHashCacheKey(gwTxHash: string) {
  return `${TX_HASH_MAPPING_PREFIX_KEY}:gw:${gwTxHash}`;
}

export class TxHashMapping {
  private store: Store;

  constructor(store: Store) {
    this.store = store;
  }

  async save(ethTxHash: Hash, gwTxHash: Hash) {
    const ethTxHashKey = ethTxHashCacheKey(ethTxHash);
    await this.store.insert(
      ethTxHashKey,
      gwTxHash,
      TX_HASH_MAPPING_CACHE_EXPIRED_TIME_MILSECS
    );
    const gwTxHashKey = gwTxHashCacheKey(gwTxHash);
    await this.store.insert(
      gwTxHashKey,
      ethTxHash,
      TX_HASH_MAPPING_CACHE_EXPIRED_TIME_MILSECS
    );
  }

  async getEthTxHash(gwTxHash: Hash): Promise<Hash | null> {
    const gwTxHashKey = gwTxHashCacheKey(gwTxHash);
    return await this.store.get(gwTxHashKey);
  }

  async getGwTxHash(ethTxHash: Hash): Promise<Hash | null> {
    const ethTxHashKey = ethTxHashCacheKey(ethTxHash);
    return await this.store.get(ethTxHashKey);
  }
}

export async function gwTxHashToEthTxHash(
  gwTxHash: HexString,
  query: Query,
  cacheStore: Store
) {
  let ethTxHashInCache = await new TxHashMapping(cacheStore).getEthTxHash(
    gwTxHash
  );
  if (ethTxHashInCache != null) {
    return ethTxHashInCache;
  }

  // query from database
  const gwTxHashInDb: Hash | undefined = await query.getEthTxHashByGwTxHash(
    gwTxHash
  );
  return gwTxHashInDb;
}

export async function ethTxHashToGwTxHash(
  ethTxHash: HexString,
  query: Query,
  cacheStore: Store
) {
  let gwTxHashInCache = await new TxHashMapping(cacheStore).getGwTxHash(
    ethTxHash
  );
  if (gwTxHashInCache != null) {
    return gwTxHashInCache;
  }

  // query from database
  const ethTxHashInDb: Hash | undefined = await query.getGwTxHashByEthTxHash(
    ethTxHash
  );
  return ethTxHashInDb;
}
