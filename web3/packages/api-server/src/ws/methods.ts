import { EthNewHead } from "../base/types/api";
import { BlockEmitter } from "../block-emitter";
import { INVALID_PARAMS, METHOD_NOT_FOUND } from "../methods/error-code";
import {
  instantFinalityHackMethods,
  methods as compatibleMethods,
} from "../methods/index";
import { middleware as wsrpc } from "./wss";
import crypto from "crypto";
import { HexNumber } from "@ckb-lumos/base";
import { Log, LogQueryOption, toApiLog } from "../db/types";
import { filterLogsByAddress, filterLogsByTopics, Query } from "../db";
import { Store } from "../cache/store";
import { CACHE_EXPIRED_TIME_MILSECS } from "../cache/constant";
import {
  wsApplyBatchRateLimitByIp,
  wsApplyRateLimitByIp,
  wsBatchLimit,
} from "../rate-limit";
import { gwTxHashToEthTxHash } from "../cache/tx-hash";
import { isInstantFinalityHackMode } from "../util";

const query = new Query();
const cacheStore = new Store(true, CACHE_EXPIRED_TIME_MILSECS);

const newrelic = require("newrelic");

const blockEmitter = new BlockEmitter();
blockEmitter.startWorker();

export function wrapper(ws: any, req: any) {
  // this function gets called on each connection

  wsrpc(ws);

  // check if use most compatible or enable additional feature
  let methods = compatibleMethods;
  if (isInstantFinalityHackMode(req)) {
    methods = instantFinalityHackMethods;
  }

  // 1. RPC request
  for (const [method, methodFunc] of Object.entries(methods)) {
    ws.on(method, async function (...args: any[]) {
      const execMethod = async () => {
        const params = args.slice(0, args.length - 1);
        const cb = args[args.length - 1];

        // check rate limit
        const err = await wsApplyRateLimitByIp(req, method);
        if (err != null) {
          return cb(err);
        }

        (methodFunc as any)(params, cb);
      };

      // add web transaction for websocket request
      return newrelic.startWebTransaction(`/ws#${method}`, async () => {
        newrelic.getTransaction();
        try {
          execMethod();
        } catch (error) {
          throw error;
        } finally {
          newrelic.endTransaction();
        }
      });
    });
  }

  // 2. RPC batch request
  ws.on("@batchRequests", async function (...args: any[]) {
    const objs = args.slice(0, args.length - 1);
    const cb = args[args.length - 1];

    const callback = (err: any, result: any) => {
      return { err, result };
    };

    // check batch limit
    const errs = wsBatchLimit(objs);
    if (errs != null) {
      return cb(
        errs.map((err) => {
          return { err };
        })
      );
    }

    // check batch rate limit
    const batchErrs = await wsApplyBatchRateLimitByIp(req, objs);
    if (batchErrs != null) {
      return cb(
        batchErrs.map((err) => {
          return { err };
        })
      );
    }

    const info = await Promise.all(
      objs.map(async (obj) => {
        if (obj.method === "eth_subscribe") {
          const r = ethSubscribe(obj.params, callback);
          return r;
        } else if (obj.method === "eth_unsubscribe") {
          const r = ethUnsubscribe(obj.params, callback);
          return r;
        }
        const value = methods[obj.method];
        if (value == null) {
          return {
            err: {
              code: METHOD_NOT_FOUND,
              message: `method ${obj.method} not found!`,
            },
          };
        }
        const r = await (value as any)(obj.params, callback);
        return r;
      })
    );
    cb(info);
  });

  // 3. RPC Subscribe request
  const newHeadsIds: Set<HexNumber> = new Set();
  const syncingIds: Set<HexNumber> = new Set();
  const logsQueryMaps: Map<HexNumber, LogQueryOption> = new Map();

  const blockListener = (blocks: EthNewHead[]) => {
    blocks.forEach((block) => {
      newHeadsIds.forEach((id) => {
        const obj = {
          jsonrpc: "2.0",
          method: "eth_subscription",
          params: {
            result: block,
            subscription: id,
          },
        };
        ws.send(JSON.stringify(obj));
      });
    });
  };

  const logsListener = (_logs: string[]) => {
    const logs: Log[] = _logs.map((_log) => {
      let log = JSON.parse(_log);
      log.id = BigInt(log.id);
      log.block_number = BigInt(log.block_number);
      log.transaction_id = BigInt(log.transaction_id);
      return log;
    });
    logsQueryMaps.forEach(async (logQuery, id) => {
      const _result = filterLogsByAddress(logs, logQuery.address);
      const result = filterLogsByTopics(_result, logQuery.topics || []);

      if (result.length === 0) return;

      const obj = {
        jsonrpc: "2.0",
        method: "eth_subscription",
        params: {
          result: await Promise.all(
            result.map(async (log) => {
              const ethTxHash = await gwTxHashToEthTxHash(
                log.transaction_hash,
                query,
                cacheStore
              );
              return toApiLog(log, ethTxHash!);
            })
          ),
          subscription: id,
        },
      };
      ws.send(JSON.stringify(obj));
    });
  };

  blockEmitter.getEmitter().on("newHeads", blockListener);
  blockEmitter.getEmitter().on("logs", logsListener);

  // when close connection, unsubscribe emitter.
  ws.on("close", function (...args: any[]) {
    blockEmitter.getEmitter().off("newHeads", blockListener);
    blockEmitter.getEmitter().off("logs", logsListener);
  });

  function ethSubscribe(params: any[], cb: any) {
    const name = params[0];

    switch (name) {
      case "newHeads": {
        const id = newSubscriptionId();
        newHeadsIds.add(id);
        return cb(null, id);
      }

      case "syncing": {
        const id = newSubscriptionId();
        syncingIds.add(id);
        return cb(null, id);
      }

      case "logs": {
        const id = newSubscriptionId();
        try {
          const query = parseLogsSubParams(params);
          logsQueryMaps.set(id, query);
          return cb(null, id);
        } catch (error) {
          return cb({
            code: INVALID_PARAMS,
            message: `no logs in params for "${name}" subscription method`,
          });
        }
      }

      default:
        return cb({
          code: METHOD_NOT_FOUND,
          message: `no "${name}" subscription in eth namespace`,
        });
    }
  }

  ws.on("eth_subscribe", function (...args: any[]) {
    const params = args.slice(0, args.length - 1);
    const cb = args[args.length - 1];

    return ethSubscribe(params, cb);
  });

  function ethUnsubscribe(params: any[], cb: any) {
    const id = params[0];
    const result =
      newHeadsIds.delete(id) ||
      syncingIds.delete(id) ||
      logsQueryMaps.delete(id);

    cb(null, result);
  }

  ws.on("eth_unsubscribe", function (...args: any[]) {
    const params = args.slice(0, args.length - 1);
    const cb = args[args.length - 1];

    return ethUnsubscribe(params, cb);
  });

  function newSubscriptionId(): HexNumber {
    return "0x" + crypto.randomBytes(16).toString("hex");
  }

  function parseLogsSubParams(params: any[]): LogQueryOption {
    if (params[0] !== "logs") {
      throw new Error("invalid params");
    }

    if (params[1] && typeof params[1] !== "object") {
      throw new Error("invalid params");
    }

    if (params[1]) {
      const query = {
        address: params[1].address,
        topics: params[1].topics,
      };
      return query;
    }

    return {};
  }
}
