import { AccessGuard } from "./cache/guard";
import { LIMIT_EXCEEDED } from "./methods/error-code";
import { Request, Response, NextFunction } from "express";
import { logger } from "./base/logger";
import { JSONRPCError } from "jayson";

export const accessGuard = new AccessGuard();

export async function wsApplyBatchRateLimitByIp(
  req: Request,
  objs: any[]
): Promise<JSONRPCError[] | undefined> {
  const ip = getIp(req);
  const methods = Object.keys(accessGuard.rpcMethods);
  if (methods.length === 0) {
    return undefined;
  }

  for (const targetMethod of methods) {
    const count = calcMethodCount(objs, targetMethod);
    if (count > 0 && ip != null) {
      const isExist = await accessGuard.isExist(targetMethod, ip);
      if (!isExist) {
        await accessGuard.add(targetMethod, ip);
      }

      const isOverRate = await accessGuard.isOverRate(targetMethod, ip, count);
      if (isOverRate) {
        const remainSecs = await accessGuard.getKeyTTL(targetMethod, ip);
        const message = `Too Many Requests, IP: ${ip}, please wait ${remainSecs}s and retry. RPC method: ${targetMethod}.`;
        const error: JSONRPCError = {
          code: LIMIT_EXCEEDED,
          message: message,
        };

        logger.debug(
          `WS Batch Rate Limit Exceed, ip: ${ip}, method: ${targetMethod}, ttl: ${remainSecs}s`
        );

        return new Array(objs.length).fill(error);
      } else {
        await accessGuard.updateCount(targetMethod, ip, count);
      }
    }
    // continue next loop
  }

  return undefined;
}

export async function wsApplyRateLimitByIp(
  req: Request,
  method: string
): Promise<JSONRPCError | undefined> {
  const ip = getIp(req);
  const methods = Object.keys(accessGuard.rpcMethods);
  if (methods.includes(method) && ip != null) {
    const res = await wsRateLimit(method, ip);
    if (res != null) {
      return res.error;
    }
  }
  return undefined;
}

export async function applyRateLimitByIp(
  req: Request,
  res: Response,
  next: NextFunction
) {
  // check batch limit
  if (batchLimit(req, res)) {
    return;
  }

  const methods = Object.keys(accessGuard.rpcMethods);
  if (methods.length === 0) {
    return next();
  }

  let isResSent = false;
  for (const method of methods) {
    const ip = getIp(req);
    const isBan = await rateLimit(req, res, method, ip);

    if (isBan) {
      // if one method is ban, we refuse all
      isResSent = true;
      break;
    }
  }

  if (!isResSent) {
    next();
  }
}

export function batchLimit(req: Request, res: Response) {
  let isBan = false;
  if (isBatchLimit(req.body)) {
    isBan = true;
    // if reach batch limit, we reject the whole req with error
    const message = `Too Many Batch Requests ${req.body.length}, limit: ${accessGuard.batchLimit}.`;
    const error = {
      code: LIMIT_EXCEEDED,
      message: message,
    };

    logger.debug(
      `Batch Limit Exceed, ${req.body.length}, limit: ${accessGuard.batchLimit}`
    );

    const content = req.body.map((b: any) => {
      return {
        jsonrpc: "2.0",
        id: b.id,
        error: error,
      };
    });

    const httpRateLimitCode = 429;
    res.status(httpRateLimitCode).send(content);
  }
  return isBan;
}

export function wsBatchLimit(body: any): JSONRPCError[] | undefined {
  if (isBatchLimit(body)) {
    // if reach batch limit, we reject the whole req with error
    const message = `Too Many Batch Requests ${body.length}, limit: ${accessGuard.batchLimit}.`;
    const error: JSONRPCError = {
      code: LIMIT_EXCEEDED,
      message: message,
    };

    logger.debug(
      `WS Batch Limit Exceed, ${body.length}, limit: ${accessGuard.batchLimit}`
    );

    return new Array(body.length).fill(error);
  }

  return undefined;
}

export async function rateLimit(
  req: Request,
  res: Response,
  rpcMethod: string,
  reqId: string | undefined
) {
  let isBan = false;
  const count = calcMethodCount(req.body, rpcMethod);
  if (count > 0 && reqId != null) {
    const isExist = await accessGuard.isExist(rpcMethod, reqId);
    if (!isExist) {
      await accessGuard.add(rpcMethod, reqId);
    }

    const isOverRate = await accessGuard.isOverRate(rpcMethod, reqId, count);
    if (isOverRate) {
      isBan = true;

      const remainSecs = await accessGuard.getKeyTTL(rpcMethod, reqId);
      const remainMilsecs = remainSecs * 1000;
      const httpRateLimitCode = 429;
      const httpRateLimitHeader = {
        "Retry-After": remainMilsecs.toString(),
      };

      const message = `Too Many Requests, IP: ${reqId}, please wait ${remainSecs}s and retry. RPC method: ${rpcMethod}.`;
      const error = {
        code: LIMIT_EXCEEDED,
        message: message,
      };

      logger.debug(
        `Rate Limit Exceed, ip: ${reqId}, method: ${rpcMethod}, ttl: ${remainSecs}s`
      );

      const content = Array.isArray(req.body)
        ? req.body.map((b) => {
            return {
              jsonrpc: "2.0",
              id: b.id,
              error: error,
            };
          })
        : {
            jsonrpc: "2.0",
            id: req.body.id,
            error: error,
          };
      res.status(httpRateLimitCode).header(httpRateLimitHeader).send(content);
    } else {
      await accessGuard.updateCount(rpcMethod, reqId, count);
    }
  }
  return isBan;
}

export async function wsRateLimit(
  rpcMethod: string,
  reqId: string
): Promise<{ error: JSONRPCError; remainSecs: number } | undefined> {
  const isExist = await accessGuard.isExist(rpcMethod, reqId);
  if (!isExist) {
    await accessGuard.add(rpcMethod, reqId);
  }

  const isOverRate = await accessGuard.isOverRate(rpcMethod, reqId);
  if (isOverRate) {
    const remainSecs = await accessGuard.getKeyTTL(rpcMethod, reqId);

    const message = `Too Many Requests, IP: ${reqId}, please wait ${remainSecs}s and retry. RPC method: ${rpcMethod}.`;
    const error: JSONRPCError = {
      code: LIMIT_EXCEEDED,
      message: message,
    };

    logger.debug(
      `WS Rate Limit Exceed, ip: ${reqId}, method: ${rpcMethod}, ttl: ${remainSecs}s`
    );
    return { error, remainSecs };
  } else {
    await accessGuard.updateCount(rpcMethod, reqId);
  }
  return undefined;
}

export function isBatchLimit(body: any) {
  if (Array.isArray(body)) {
    return body.length >= accessGuard.batchLimit;
  }
  return false;
}

export function calcMethodCount(body: any, targetMethod: string): number {
  if (Array.isArray(body)) {
    return body.filter((b) => b.method === targetMethod).length;
  }

  return body.method === targetMethod ? 1 : 0;
}

export function getIp(req: Request) {
  let ip;
  if (req.headers["x-forwarded-for"] != null) {
    ip = (req.headers["x-forwarded-for"] as string).split(",")[0].trim();
  }

  return ip || req.socket.remoteAddress;
}
