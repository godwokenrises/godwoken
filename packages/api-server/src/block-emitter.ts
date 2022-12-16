import { envConfig } from "./base/env-config";
import { logger } from "./base/logger";
import { Query } from "./db";
import { EventEmitter } from "events";
import { toApiNewHead } from "./db/types";
import cluster from "cluster";
import * as Sentry from "@sentry/node";
import { Store } from "./cache/store";
import {
  TIP_BLOCK_HASH_CACHE_KEY,
  TIP_BLOCK_HASH_CACHE_EXPIRED_TIME_MS,
} from "./cache/constant";

const newrelic = require("newrelic");

// init cache
const cacheStore: Store = new Store(true, TIP_BLOCK_HASH_CACHE_EXPIRED_TIME_MS);

// Only start in main worker, and `startWorker` in workers to get emitter.
export class BlockEmitter {
  private query: Query;
  private isRunning: boolean;
  private currentTip: bigint;
  private emitter: EventEmitter;
  private livenessCheckIntervalSeconds: number;

  constructor({ livenessCheckIntervalSeconds = 5 } = {}) {
    this.query = new Query();
    this.isRunning = false;
    this.currentTip = -1n;
    this.emitter = new EventEmitter();
    this.livenessCheckIntervalSeconds = livenessCheckIntervalSeconds;
  }

  // Main worker
  async startForever() {
    await this.start();
    setInterval(async () => {
      if (!this.running()) {
        logger.error("BlockEmitter has stopped, maybe check the log?");
        await this.start();
      }
    }, this.livenessCheckIntervalSeconds * 1000);
  }

  async start() {
    this.isRunning = true;
    const currentTip: bigint | undefined = await this.query.getTipBlockNumber();

    if (currentTip != null) {
      this.currentTip = currentTip;
    }
    this.scheduleLoop();
  }

  // cluster workers
  startWorker() {
    process.on("message", (msg: any) => {
      const type = msg.type;
      const content = msg.content;
      if (type === "BlockEmitter#newHeads") {
        this.emitter.emit("newHeads", content);
      } else if (type === "BlockEmitter#logs") {
        this.emitter.emit("logs", content);
      }
    });
  }

  stop() {
    this.isRunning = false;
  }

  running() {
    return this.isRunning;
  }

  scheduleLoop(timeout = 1) {
    setTimeout(() => {
      this.loop();
    }, timeout);
  }

  loop() {
    if (!this.running()) {
      return;
    }
    this.poll()
      .then((timeout) => {
        this.scheduleLoop(timeout);
      })
      .catch((e) => {
        logger.error(`Error occurs: ${e} ${e.stack}, stopping emit newHeads!`);
        if (envConfig.sentryDns) {
          Sentry.captureException(e);
        }
        this.stop();
      });
  }

  async poll() {
    let timeout = 1000;
    const tip = await this.query.getTipBlockNumber();
    if (tip == null || this.currentTip >= tip) {
      return timeout;
    }

    const executePoll = async () => {
      const min = this.currentTip;
      const max = tip;

      const blocks = await this.query.getBlocksByNumbers(min, max);
      const newHeads = blocks.map((b) => toApiNewHead(b));
      this.notify("newHeads", newHeads);

      // update tipBlockHash in cache
      cacheStore.insert(
        TIP_BLOCK_HASH_CACHE_KEY,
        newHeads[newHeads.length - 1].hash
      );

      const logs = await this.query.getLogsByFilter({
        fromBlock: min + BigInt(1),
        toBlock: max,
        addresses: [],
        topics: [],
      });

      const newLogs = logs.map((log) =>
        JSON.stringify(
          log,
          (_key, value) =>
            typeof value === "bigint" ? value.toString() : value // return everything else unchanged
        )
      );
      if (logs.length > 0) {
        this.notify("logs", newLogs);
      }

      this.currentTip = tip;
      return timeout;
    };

    // add new relic background transaction
    return newrelic.startBackgroundTransaction(
      `BlockEmitter#pool`,
      async () => {
        newrelic.getTransaction();
        try {
          return await executePoll();
        } catch (error) {
          throw error;
        } finally {
          newrelic.endTransaction();
        }
      }
    );
  }

  getEmitter(): EventEmitter {
    return this.emitter;
  }

  private notify(type: string, content: any) {
    const workers = cluster.workers!;
    for (const workerId in workers) {
      const worker = workers[workerId];
      if (worker) {
        worker.send({
          type: `BlockEmitter#${type}`,
          content,
        });
      }
    }
  }
}
