require("newrelic");
import { Store } from "./store";
import { HexString } from "@ckb-lumos/base";
import fs from "fs";
import path from "path";
import { CACHE_EXPIRED_TIME_MILSECS } from "./constant";
import { logger } from "../base/logger";

const RedisPrefixName = "access";
const configPath = path.resolve(__dirname, "../../rate-limit-config.json");

export const EXPIRED_TIME_MILSECS = 1 * 60 * 1000; // milsec, default 1 minutes
export const MAX_REQUEST_COUNT = 30;

export interface RateLimitConfig {
  expired_time_milsec: number;
  methods: RpcMethodLimit;
}

export interface RpcMethodLimit {
  [reqMethod: string]: number; // max rpc method request counts in expired_time
}

export function getRateLimitConfig() {
  if (fs.existsSync(configPath)) {
    // todo: validate config
    const config: RateLimitConfig = require(configPath);
    return config;
  }

  // default config, no rpc method apply rate limit
  return {
    expired_time_milsec: EXPIRED_TIME_MILSECS,
    methods: {},
  } as RateLimitConfig;
}

export class AccessGuard {
  public store: Store;
  public rpcMethods: RpcMethodLimit;
  public expiredTimeMilsecs: number;

  constructor(
    enableExpired = true,
    expiredTimeMilsecs?: number, // milsec, default 1 minutes
    store?: Store
  ) {
    const config = getRateLimitConfig();
    logger.debug("rate-limit-config:", config);
    expiredTimeMilsecs = expiredTimeMilsecs || config.expired_time_milsec;
    this.store = store || new Store(enableExpired, expiredTimeMilsecs);
    this.rpcMethods = config.methods;
    this.expiredTimeMilsecs = expiredTimeMilsecs || CACHE_EXPIRED_TIME_MILSECS;
  }

  async setMaxReqLimit(rpcMethod: string, maxReqCount: number) {
    this.rpcMethods[rpcMethod] = maxReqCount;
  }

  async getCount(rpcMethod: string, reqId: string) {
    const id = getId(rpcMethod, reqId);
    const count = await this.store.get(id);
    if (count == null) {
      return null;
    }
    return +count;
  }

  async add(rpcMethod: string, reqId: string): Promise<HexString | undefined> {
    const isExist = await this.isExist(rpcMethod, reqId);
    if (!isExist) {
      const id = getId(rpcMethod, reqId);
      await this.store.insert(id, 0);
      return id;
    }
  }

  async updateCount(rpcMethod: string, reqId: string) {
    const isExist = await this.isExist(rpcMethod, reqId);
    if (isExist === true) {
      const id = getId(rpcMethod, reqId);
      await this.store.incr(id);
    }
  }

  async isExist(rpcMethod: string, reqId: string) {
    const id = getId(rpcMethod, reqId);
    const data = await this.store.get(id);
    if (data == null) return false;
    return true;
  }

  async isOverRate(rpcMethod: string, reqId: string): Promise<boolean> {
    const id = getId(rpcMethod, reqId);
    const data = await this.store.get(id);
    if (data == null) return false;
    if (this.rpcMethods[rpcMethod] == null) return false;

    const count = +data;
    const maxNumber = this.rpcMethods[rpcMethod];
    if (count > maxNumber) {
      return true;
    }
    return false;
  }

  async getKeyTTL(rpcMethod: string, reqId: string) {
    const id = getId(rpcMethod, reqId);
    const remainSecs = await this.store.ttl(id);
    if (remainSecs === -1) {
      const value = (await this.store.get(id)) || "0";
      logger.info(
        `key ${id} with no ttl, reset: ${this.expiredTimeMilsecs}ms, ${value}`
      );
      await this.store.insert(id, value, this.expiredTimeMilsecs / 1000);
      return await this.store.ttl(id);
    }
    return remainSecs;
  }
}

export function getId(rpcMethod: string, reqUniqueId: string): HexString {
  return `${RedisPrefixName}.${rpcMethod}.${reqUniqueId}`;
}
