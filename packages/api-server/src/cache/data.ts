import { handleGwError } from "../methods/gw-error";

require("newrelic");
import { createClient } from "redis";
import { envConfig } from "../base/env-config";
import crypto from "crypto";
import { logger } from "../base/logger";

// note: Subscribing to a channel requires a dedicated stand-alone connection
// init publisher redis client
export const pubClient = createClient({
  url: envConfig.redisUrl,
});
pubClient.connect();
pubClient.on("error", (err) => logger.error("Redis Client Error", err));
// init subscriber redis client
export const subClient = createClient({
  url: envConfig.redisUrl,
});
subClient.connect();
subClient.on("error", (err) => logger.error("Redis Client Error", err));

export const SUB_TIME_OUT_MS = 2 * 1000; // 2s;
export const LOCK_KEY_EXPIRED_TIME_OUT_MS = 60 * 1000; // 60s, the max tolerate timeout for execute call
export const DATA_KEY_EXPIRED_TIME_OUT_MS = 5 * 60 * 1000; // 5 minutes
export const POLL_INTERVAL_MS = 50; // 50ms
export const POLL_TIME_OUT_MS = 2 * 60 * 1000; // 2 minutes

export const DEFAULT_PREFIX_NAME = "defaultDataCache";
export const DEFAULT_IS_ENABLE_LOCK = true;

export interface DataCacheConstructor {
  rawDataKey: string;
  executeCallResult: ExecuteCallResult;
  prefixName?: string;
  isLockEnable?: boolean;
  lock?: Partial<RedisLock>;
  dataKeyExpiredTimeOutMs?: number;
}

export type ExecuteCallResult = () => Promise<string>;

export interface RedisLock {
  key: LockKey;
  subscribe: RedSubscribe;
  pollIntervalMs: number;
  pollTimeOutMs: number;
}

export interface LockKey {
  name: string;
  expiredTimeMs: number;
}

export interface RedSubscribe {
  channel: string;
  timeOutMs: number;
}

export class RedisDataCache {
  public prefixName: string;
  public rawDataKey: string; // unique part of dataKey
  public dataKey: string; // real dataKey saved on redis combined from rawDataKey with prefix name and so on.
  public lock: RedisLock | undefined;
  public dataKeyExpiredTimeOut: number;
  public executeCallResult: ExecuteCallResult;

  constructor(args: DataCacheConstructor) {
    this.prefixName = args.prefixName || DEFAULT_PREFIX_NAME;
    this.rawDataKey = args.rawDataKey;
    this.dataKey = `${this.prefixName}:key:${this.rawDataKey}`;
    this.executeCallResult = args.executeCallResult;
    this.dataKeyExpiredTimeOut =
      args.dataKeyExpiredTimeOutMs || DATA_KEY_EXPIRED_TIME_OUT_MS;

    const isLockEnable = args.isLockEnable ?? DEFAULT_IS_ENABLE_LOCK; // default is true;
    if (isLockEnable) {
      this.lock = {
        key: {
          name:
            args.lock?.key?.name ||
            `${this.prefixName}:lock:${this.rawDataKey}`,
          expiredTimeMs:
            args.lock?.key?.expiredTimeMs || LOCK_KEY_EXPIRED_TIME_OUT_MS,
        },
        subscribe: {
          channel:
            args.lock?.subscribe?.channel ||
            `${this.prefixName}:channel:${this.rawDataKey}`,
          timeOutMs: args.lock?.subscribe?.timeOutMs || SUB_TIME_OUT_MS,
        },
        pollIntervalMs: args.lock?.pollIntervalMs || POLL_INTERVAL_MS,
        pollTimeOutMs: args.lock?.pollTimeOutMs || POLL_TIME_OUT_MS,
      };
    }
  }

  async get() {
    const dataKey = this.dataKey;
    const value = await pubClient.get(dataKey);
    if (value != null) {
      logger.debug(
        `[${this.constructor.name}]: hit cache via Redis.Get, key: ${dataKey}`
      );
      return value;
    }

    const setDataKeyOptions = { PX: this.dataKeyExpiredTimeOut };

    if (this.lock == null) {
      const result = await this.executeCallResult();
      // set data cache
      await pubClient.set(dataKey, result, setDataKeyOptions);
      return result;
    }

    // use redis-lock for data cache
    const t1 = new Date();
    const lockValue = getLockUniqueValue();
    const expiredTimeMs = this.lock.key.expiredTimeMs;

    const releaseLock = async (lockValue: string) => {
      if (!this.lock) throw new Error("enable lock first!");

      const value = await pubClient.get(this.lock.key.name);
      if (value === lockValue) {
        // only lock owner can delete the lock
        const delNumber = await pubClient.del(this.lock.key.name);
        logger.debug(
          `[${this.constructor.name}]: delete key ${this.lock.key.name}, result: ${delNumber}`
        );
      }
    };

    while (true) {
      const value = await pubClient.get(dataKey);
      if (value != null) {
        logger.debug(
          `[${this.constructor.name}]: hit cache via Redis.Get, key: ${dataKey}`
        );
        return value;
      }

      const isLockAcquired = await pubClient.set(
        this.lock.key.name,
        lockValue,
        {
          NX: true,
          PX: expiredTimeMs,
        }
      );

      if (isLockAcquired) {
        try {
          const result = await this.executeCallResult();
          // set data cache
          await pubClient.set(dataKey, result, setDataKeyOptions);
          // publish the result to channel
          const publishResult = successResult(result);
          const totalSubs = await pubClient.publish(
            this.lock.subscribe.channel,
            publishResult
          );
          logger.debug(
            `[${this.constructor.name}][success]: publish message ${publishResult} on channel ${this.lock.subscribe.channel}, total subscribers: ${totalSubs}`
          );
          await releaseLock(lockValue);
          return result;
        } catch (error: any) {
          const reason = error.message;
          if (!reason.includes("request to")) {
            // publish the non-network-connecting-error-result to channel
            const publishResult = errorResult(reason);
            const totalSubs = await pubClient.publish(
              this.lock.subscribe.channel,
              publishResult
            );
            logger.debug(
              `[${this.constructor.name}][error]: publish message ${publishResult} on channel ${this.lock.subscribe.channel}, total subscribers: ${totalSubs}`
            );
          }
          await releaseLock(lockValue);
          throw error;
        }
      }

      // if lock is not acquired
      try {
        const result = await this.subscribe();
        logger.debug(
          `[${this.constructor.name}]: hit cache via Redis.Subscribe, key: ${dataKey}`
        );
        return result;
      } catch (error: any) {
        if (
          !JSON.stringify(error).includes(
            "subscribe channel for message time out"
          )
        ) {
          logger.debug(
            `[${this.constructor.name}]: subscribe err:`,
            error.message
          );
          throw error;
        }
      }

      // check if poll time out
      const t2 = new Date();
      const diff = t2.getTime() - t1.getTime();
      if (diff > this.lock.pollTimeOutMs) {
        throw new Error(
          `poll data value from cache layer time out ${this.lock.pollTimeOutMs}`
        );
      }

      await asyncSleep(this.lock.pollIntervalMs);
    }
  }

  async subscribe() {
    if (this.lock == null) {
      throw new Error(`enable redis lock first!`);
    }

    const p = new Promise((resolve, reject) => {
      subClient.subscribe(
        this.lock!.subscribe.channel,
        async (message: string) => {
          try {
            const data = parseExecuteResult(message);
            await subClient.unsubscribe(this.lock!.subscribe.channel);
            return resolve(data);
          } catch (error) {
            return reject(error);
          }
        }
      );
    });

    const t = new Promise((_resolve, reject) => {
      setTimeout(() => {
        subClient.unsubscribe(this.lock!.subscribe.channel);
        return reject(
          `subscribe channel for message time out ${this.lock?.subscribe.timeOutMs}`
        );
      }, this.lock?.subscribe.timeOutMs);
    });

    return (await Promise.race([p, t])) as Promise<string>;
  }
}

export function getLockUniqueValue() {
  return "0x" + crypto.randomBytes(20).toString("hex");
}

export enum PublishExecuteResultStatus {
  Success,
  Error,
}

export interface PublishExecuteResult {
  status: PublishExecuteResultStatus;
  data?: string;
  err?: string;
}

export function successResult(result: string) {
  const res: PublishExecuteResult = {
    status: PublishExecuteResultStatus.Success,
    data: result,
  };
  return JSON.stringify(res);
}

export function errorResult(reason: string) {
  const res: PublishExecuteResult = {
    status: PublishExecuteResultStatus.Error,
    err: reason,
  };
  return JSON.stringify(res);
}

export function parseExecuteResult(res: string) {
  const executionResult = JSON.parse(res);
  if (executionResult?.status === PublishExecuteResultStatus.Success) {
    return (executionResult as PublishExecuteResult).data;
  }
  if (executionResult?.err != null) {
    handleGwError(executionResult.err);
  }

  throw new Error("[RedisSubscribeResult] unrecognizable result: " + res);
}

const asyncSleep = async (ms = 0) => {
  return new Promise((r) => setTimeout(() => r("ok"), ms));
};
