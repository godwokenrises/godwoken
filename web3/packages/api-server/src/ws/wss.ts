import { envConfig } from "../base/env-config";
import { logger } from "../base/logger";
import { METHOD_NOT_FOUND } from "../methods/error-code";
import { methods } from "../methods/index";
import { JSONRPCVersionTwoRequest } from "jayson";

const wsRpcMethods = Object.keys(methods).concat(
  "eth_subscribe",
  "eth_unsubscribe"
);

export function middleware(ws: any) {
  ws.on("message", dispatch);
  ws.on("data", dispatch);

  function dispatch(msg: string) {
    try {
      const obj = JSON.parse(msg.toString());

      logRequest(obj);

      if (Array.isArray(obj)) {
        const args = ["@batchRequests" as any].concat(obj, [
          (info: any[]) => batchResponder(obj, info),
        ]);
        ws.emit.apply(ws, args);
        return;
      }

      // check if method allow
      if (!wsRpcMethods.includes(obj.method)) {
        const err = {
          code: METHOD_NOT_FOUND,
          message: `method ${obj.method} not found!`,
        };
        return responder(obj, err, null);
      }

      const args = [obj.method].concat(obj.params, [
        (err: any, result: any) => responder(obj, err, result),
      ]);
      ws.emit.apply(ws, args);
    } catch {
      ws.close();
    }
  }

  function responder(obj: any, err: any, result: any) {
    const respObj: JsonRpcRequestResult = {
      id: obj.id,
      jsonrpc: "2.0",
    };
    if (err == null) {
      respObj.result = result;
    } else {
      respObj.error = err;
    }
    const resp = JSON.stringify(respObj);
    ws.send(resp);
  }

  function batchResponder(objs: any[], info: any[]) {
    const respObjs = objs.map((o, i) => {
      const { err, result } = info[i];
      const respObj: JsonRpcRequestResult = {
        id: o.id,
        jsonrpc: "2.0",
      };
      if (err == null) {
        respObj.result = result;
      } else {
        respObj.error = err;
      }
      return respObj;
    });

    const resp = JSON.stringify(respObjs);
    ws.send(resp);
  }

  function logRequest(
    obj: JSONRPCVersionTwoRequest | Array<JSONRPCVersionTwoRequest>
  ) {
    if (envConfig.logRequestBody) {
      return logger.info("websocket request.body:", obj);
    }

    if (Array.isArray(obj)) {
      return logger.info(
        "websocket request.method:",
        obj.map((o) => o.method)
      );
    }

    return logger.info("websocket request.method:", obj.method);
  }
}

interface JsonRpcRequestResult {
  id: any;
  jsonrpc: "2.0";
  error?: any;
  result?: any;
}
