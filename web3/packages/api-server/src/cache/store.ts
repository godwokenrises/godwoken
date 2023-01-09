require("newrelic");
import { RedisClientType } from "redis";
import { CACHE_EXPIRED_TIME_MILSECS } from "../cache/constant";
import { globalClient, SetOptions } from "./redis";

export class Store {
  private client: RedisClientType;
  private setOptions: SetOptions;

  constructor(enableExpired?: boolean, keyExpiredTimeMilSecs?: number) {
    this.client = globalClient;
    if (enableExpired == null) {
      enableExpired = false;
    }

    this.setOptions = enableExpired
      ? {
          PX: keyExpiredTimeMilSecs || CACHE_EXPIRED_TIME_MILSECS,
        }
      : {};
  }

  async insert(
    key: string,
    value: string | number,
    expiredTimeMilSecs?: number
  ) {
    let setOptions = this.setOptions;
    const PX = expiredTimeMilSecs || this.setOptions.PX;
    if (PX) {
      setOptions.PX = PX;
    }

    return await this.client.set(key, value.toString(), setOptions);
  }

  async delete(key: string) {
    // use unlink instead of DEL to avoid blocking
    return await this.client.unlink(key);
  }

  async get(key: string) {
    return await this.client.get(key);
  }

  async size() {
    return await this.client.dbSize();
  }

  async addSet(name: string, members: string | string[]) {
    return await this.client.sAdd(name, members);
  }

  async incr(key: string) {
    return await this.client.incr(key);
  }

  async incrBy(key: string, offset: number) {
    const data = await this.client.get(key);
    if (data == null) {
      throw new Error("can not update before key exits");
    }
    if (isNaN(data as any)) {
      throw new Error("can not update with NaN value");
    }
    return await this.client.incrBy(key, offset);
  }

  async ttl(key: string) {
    return await this.client.ttl(key);
  }
}
