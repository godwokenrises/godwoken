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
export const BATCH_LIMIT = 100000; // 100_000 RPCs in single batch req

const RATE_LIMIT_SCRIPT =
  "local current = redis.call('incrby', KEYS[1], ARGV[1]); local pttl = redis.call('pttl', KEYS[1]); if pttl < 0 then redis.call('pexpire', KEYS[1], ARGV[2]) end; return {current, pttl}";

export interface RateLimitConfig {
  batch_limit: number;
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
  public batchLimit: number;

  private evalSha?: string;

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
    this.batchLimit = config.batch_limit || BATCH_LIMIT;

    this.store.scriptLoad(RATE_LIMIT_SCRIPT).then((evalSha) => {
      this.evalSha = evalSha;
      logger.info("AccessGuard eval sha:", evalSha);
    });
  }

  private async getEvalSha() {
    if (!this.evalSha) {
      this.evalSha = await this.store.scriptLoad(RATE_LIMIT_SCRIPT);
      logger.warn(`AccessGuard reget eval sha:`, this.evalSha);
    }
    return this.evalSha;
  }

  async limitApiCall(
    rpcMethod: string,
    reqId: string,
    offset: number = 1
  ): Promise<
    | {
        current: number;
        pttl: number;
        isOverRate: boolean;
      }
    | {
        isOverRate: false;
      }
  > {
    const id = getId(rpcMethod, reqId);

    const maxNumber: number | undefined = this.rpcMethods[rpcMethod];

    // No limit for this RPC
    if (maxNumber == null) {
      return {
        isOverRate: false,
      };
    }

    const result: [string, string] = (await this.store.evalshaRetry(
      RATE_LIMIT_SCRIPT,
      await this.getEvalSha(),
      [id],
      [offset.toString(), this.expiredTimeMilsecs.toString()]
    )) as [string, string];
    const current = +result[0];
    const pttl = +result[1];

    // if pttl < 0 (-1), means expired before eval call.
    const isOverRate: boolean = current > maxNumber && pttl > 0;

    return {
      current,
      pttl,
      isOverRate,
    };
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
}

export function getId(rpcMethod: string, reqUniqueId: string): HexString {
  return `${RedisPrefixName}.${rpcMethod}.${reqUniqueId}`;
}
